//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!
//! todo as of 2023-02-16:
//!     use a thread to create/delete MediaTracks in response to a channel command. see manage_tracks at the bottom of the file
//!     create signal handling functions

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::Stream;
use futures::StreamExt;
use ipfs::libp2p::gossipsub::GossipsubMessage;
use ipfs::Ipfs;
use ipfs::IpfsTypes;
use ipfs::SubscriptionStream;
use serde::Deserialize;
use serde::Serialize;
use simple_webrtc::audio;
use simple_webrtc::events::EmittedEvents;
use simple_webrtc::events::WebRtcEventStream;
use simple_webrtc::Controller;
use simple_webrtc::MediaSourceId;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use uuid::Uuid;
use warp::blink::BlinkEventKind;
use warp::blink::MimeType;
use warp::libipld;
use warp::multipass::MultiPass;
use warp::sata::Sata;
use warp::sync::RwLock;
use warp::{
    blink::{Blink, BlinkEventStream},
    crypto::DID,
    error::Error,
};
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

mod media_track;
mod signaling;
mod simple_webrtc;

mod ipfs_routes {
    use uuid::Uuid;
    use warp::crypto::DID;

    const TELECON_BROADCAST: &str = "telecon";
    const OFFER_CALL: &str = "offer_call";

    /// subscribe/unsubscribe per-call
    pub fn call_broadcast_route(call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}")
    }

    /// subscribe/unsubscribe per-call
    pub fn call_signal_route(peer: &DID, call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}/{peer}")
    }

    /// subscribe to this when initializing Blink
    pub fn offer_call_route(peer: &DID) -> String {
        format!("{OFFER_CALL}/{peer}")
    }
}

// todo: add option to init WebRtc using a configuration file
pub struct WebRtc<T: IpfsTypes> {
    account: Box<dyn MultiPass>,
    ipfs: Arc<RwLock<Ipfs<T>>>,
    id: DID,
    // a tx channel which emits events to drive the UI
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    webrtc: Arc<Mutex<simple_webrtc::Controller>>,
    active_call: Option<ActiveCall>,
    pending_calls: HashMap<Uuid, Call>,
    cpal_host: cpal::Host,
    // cpal::Device.name()
    audio_input: Option<String>,
    // cpal::Device.name()
    audio_output: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Call {
    id: Uuid,
    participants: Vec<DID>,
}

#[derive(Clone)]
struct ActiveCall {
    call: Call,
    state: CallState,
    stop: Arc<Notify>,
}

#[derive(Clone)]
enum CallState {
    Pending,
    InProgress,
    Ended,
}

impl<T: IpfsTypes> Drop for WebRtc<T> {
    fn drop(&mut self) {
        if let Some(ac) = self.active_call.as_ref() {
            ac.stop.notify_waiters();
        }
    }
}

impl Call {
    fn new(participants: Vec<DID>) -> Self {
        Self {
            id: Uuid::new_v4(),
            participants,
        }
    }
}

// used when a call is offered
impl ActiveCall {
    fn new(participants: Vec<DID>) -> Self {
        Self {
            call: Call::new(participants),
            state: CallState::Pending,
            stop: Arc::new(Notify::new()),
        }
    }
}

// used when a call is accepted
impl From<Call> for ActiveCall {
    fn from(value: Call) -> Self {
        Self {
            call: value,
            state: CallState::InProgress,
            stop: Arc::new(Notify::new()),
        }
    }
}

/// sent via offer_call/<DID>
#[derive(Serialize, Deserialize)]
enum InitiationSignal {
    /// invite a peer to join a call
    Offer(Call),
    /// indicate that the peer will not be joining
    Reject(DID),
}

/// sent via telecon/<Uuid>
#[derive(Serialize, Deserialize)]
enum BroadcastSignal {
    /// Sent when a peer joins a call.
    /// Used by the peers to dial each other
    Hello,
    /// sent when a peer leaves the call
    HangUp,
}

/// sent via telecon/<Uuid>/<DID>
#[derive(Serialize, Deserialize)]
enum DirectSignal {
    /// Initiates a WebRTC connection
    Dial(RTCSessionDescription),
    /// Completes WebRTC initiation. next is ICE discovery
    Sdp(RTCSessionDescription),
    /// Send peer your ICE candidates as they are discovered
    Ice(RTCIceCandidate),
}

#[async_trait]
impl<T: IpfsTypes> Blink for WebRtc<T> {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    async fn get_event_stream(&mut self) -> Result<BlinkEventStream, Error> {
        let mut rx = self.ui_event_ch.subscribe();
        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };
        Ok(BlinkEventStream(Box::pin(stream)))
    }

    // ------ Create/Join a call ------

    /// attempt to initiate a call. Only one call may be offered at a time.
    /// cannot offer a call if another call is in progress.
    /// During a call, WebRTC connections should only be made to
    /// peers included in the Vec<DID>.
    async fn offer_call(&mut self, participants: Vec<DID>) -> Result<(), Error> {
        if let Some(_call) = self.active_call.as_ref() {
            // todo: end call
        }
        let ac = ActiveCall::new(participants);
        self.active_call = Some(ac.clone());

        self.init_call(ac.call.clone(), ac.stop).await?;

        let ipfs = self.ipfs.read();
        // send message until participants accept or decline.
        let data = Sata::default();
        let payload = ac.call.clone();
        let res = data.encode(
            libipld::IpldCodec::DagJson,
            warp::sata::Kind::Static,
            payload,
        )?;
        let bytes = match serde_cbor::to_vec(&res) {
            Ok(b) => b,
            Err(e) => {
                log::error!("failed to encode Call struct: {e}");
                return Err(Error::SerdeCborError(e));
            }
        };
        for participant in &ac.call.participants {
            if let Err(e) = ipfs
                .pubsub_publish(ipfs_routes::offer_call_route(participant), bytes.clone())
                .await
            {
                log::error!("failed to offer call to participant {participant}: {e}");
            }
            todo!();
        }

        todo!();
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(_call) = self.active_call.as_ref() {
            // todo: end call
        }

        if let Some(call) = self.pending_calls.remove(&call_id) {
            let ac: ActiveCall = call.into();
            self.active_call = Some(ac.clone());
            self.init_call(ac.call, ac.stop).await?;
        }
        todo!()
    }
    /// notify a sender/group that you will not join a call
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(mut _call) = self.pending_calls.remove(&call_id) {
            // todo: signal
        }
        todo!()
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        if let Some(ac) = self.active_call.take() {
            // todo: leave call
            ac.stop.notify_waiters();

            let mut webrtc = self.webrtc.lock().await;
            webrtc.deinit().await?;

            // todo: remove media streams
        }
        todo!()
    }

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error> {
        let device_iter = match self.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error> {
        let new_input = Some(device_name.into());
        if self.audio_input == new_input {
            return Ok(());
        }

        let host = cpal::default_host();
        let devices = match host.input_devices() {
            Ok(d) => d,
            Err(e) => {
                return Err(warp::error::Error::OtherWithContext(format!(
                    "could not get input devices: {e}"
                )));
            }
        };
        for device in devices {
            if let Ok(name) = device.name() {
                if name == device_name {
                    media_track::change_audio_input(device).await;
                    self.audio_input = new_input;
                    return Ok(());
                }
            }
        }

        Err(warp::error::Error::OtherWithContext(
            "input device not found".into(),
        ))
    }
    async fn get_available_speakers(&self) -> Result<Vec<String>, Error> {
        let device_iter = match self.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error> {
        let new_output = Some(device_name.into());
        if self.audio_output == new_output {
            return Ok(());
        }

        let host = cpal::default_host();
        let devices = match host.output_devices() {
            Ok(d) => d,
            Err(e) => {
                return Err(warp::error::Error::OtherWithContext(format!(
                    "could not get input devices: {e}"
                )));
            }
        };
        for device in devices {
            if let Ok(name) = device.name() {
                if name == device_name {
                    media_track::change_audio_output(device).await?;
                    self.audio_output = new_output;
                    return Ok(());
                }
            }
        }

        Err(warp::error::Error::OtherWithContext(
            "output device not found".into(),
        ))
    }
    async fn get_available_cameras(&self) -> Result<Vec<String>, Error> {
        todo!()
    }
    async fn select_camera(&mut self, _device_name: &str) -> Result<(), Error> {
        todo!()
    }

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn unmute_self(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn enable_camera(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn disable_camera(&mut self) -> Result<(), Error> {
        todo!()
    }
    async fn record_call(&mut self, _output_file: &str) -> Result<(), Error> {
        todo!()
    }
    async fn stop_recording(&mut self) -> Result<(), Error> {
        todo!()
    }

    // ------ Utility Functions ------

    /// Returns the ID of the current call, or None if
    /// a call is not in progress
    fn get_call_id(&self) -> Option<Uuid> {
        todo!()
    }
}

impl<T: IpfsTypes> WebRtc<T> {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Self> {
        let identity = loop {
            if let Ok(identity) = account.get_own_identity().await {
                break identity;
            }
            tokio::time::sleep(Duration::from_millis(100)).await
        };
        let did = identity.did_key();

        let ipfs_handle = match account.handle() {
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
            _ => anyhow::bail!("Unable to obtain IPFS Handle"),
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => {
                anyhow::bail!("Unable to use IPFS Handle");
            }
        };

        let webrtc = Arc::new(Mutex::new(simple_webrtc::Controller::new(did.clone())?));
        let (ui_event_ch, _rx) = broadcast::channel(1024);

        let cpal_host = cpal::platform::default_host();
        let input_device = match cpal_host.default_input_device() {
            Some(d) => {
                let name = d.name();
                media_track::change_audio_input(d).await;
                name.ok()
            }
            None => None,
        };
        let output_device = match cpal_host.default_output_device() {
            Some(d) => {
                let name = d.name();
                media_track::change_audio_output(d).await?;
                name.ok()
            }
            None => None,
        };

        let webrtc = Self {
            webrtc,
            account,
            ipfs: Arc::new(RwLock::new(ipfs.clone())),
            id: did.clone(),
            ui_event_ch,
            active_call: None,
            pending_calls: HashMap::new(),
            cpal_host,
            audio_input: input_device,
            audio_output: output_device,
        };

        if let Err(e) = ipfs
            .pubsub_subscribe(ipfs_routes::offer_call_route(&did))
            .await
        {
            log::error!("failed to subscribe to offer_call_route: {e}");
            return Err(e);
        }

        Ok(webrtc)
    }

    // todo: make sure this only gets called once
    async fn init_call(&mut self, call: Call, stop: Arc<Notify>) -> anyhow::Result<()> {
        let ipfs = self.ipfs.read();

        // use this on error conditions and after terminating the call
        let unsubscribe = async {
            if let Err(e) = ipfs
                .pubsub_unsubscribe(&ipfs_routes::call_broadcast_route(&call.id))
                .await
            {
                log::error!("failed to unsubscribe call_broadcast_route: {e}");
            }

            if let Err(e) = ipfs
                .pubsub_unsubscribe(&ipfs_routes::call_signal_route(&self.id, &call.id))
                .await
            {
                log::error!("failed to unsubscribe cal_signal_route: {e}");
            }
        };

        let call_broadcast_stream = match ipfs
            .pubsub_subscribe(ipfs_routes::call_broadcast_route(&call.id))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to subscribe to call_broadcast_route: {e}");
                return Err(e);
            }
        };

        let call_signaling_stream = match ipfs
            .pubsub_subscribe(ipfs_routes::call_signal_route(&self.id, &call.id))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to subscribe to call_signaling_route: {e}");
                unsubscribe.await;
                return Err(e);
            }
        };

        let get_event_stream = async {
            let webrtc = self.webrtc.lock().await;
            webrtc.get_event_stream()
        }
        .await;
        // this one is already pinned to the heap
        let webrtc_event_stream = match get_event_stream {
            Ok(s) => WebRtcEventStream(Box::pin(s)),
            Err(e) => {
                log::error!("failed to get webrtc_event_stream: {e}");
                unsubscribe.await;
                return Err(e);
            }
        };
        // SimpleWebRTC instance
        let webrtc = self.webrtc.clone();
        tokio::task::spawn(async move {
            handle_webrtc(
                webrtc,
                call_broadcast_stream,
                call_signaling_stream,
                webrtc_event_stream,
                stop,
            )
            .await;
        });

        Ok(())
    }
}

async fn decode_broadcast_signal(message: Arc<GossipsubMessage>) -> anyhow::Result<()> {
    let data = serde_cbor::from_slice::<Sata>(&message.data)?;
    let sdp = data.decode::<BroadcastSignal>()?;

    //todo: verify that message sender is in conversation list
    // todo: initiate WebRTC communications if needed

    todo!()
}

async fn create_source_track(
    webrtc: &Arc<Mutex<Controller>>,
    device: &cpal::Device,
    codec: RTCRtpCodecCapability,
    source_id: MediaSourceId,
) -> Result<Box<dyn audio::SourceTrack>, Box<dyn std::error::Error>> {
    let track = {
        let mut s = webrtc.lock().await;

        // a media source must be added before attempting to connect or SDP will fail
        s.add_media_source(source_id, codec.clone()).await?
    };

    // create an audio source
    let source_track = //simple_webrtc::media::OpusSource::init(input_device, track, codec)?;
     simple_webrtc::audio::create_source_track(device, track, codec)?;

    Ok(source_track)
}

async fn handle_webrtc(
    webrtc: Arc<Mutex<Controller>>,
    call_broadcast_stream: SubscriptionStream,
    call_signaling_stream: SubscriptionStream,
    mut webrtc_event_stream: WebRtcEventStream,
    stop: Arc<Notify>,
) {
    futures::pin_mut!(call_broadcast_stream);
    futures::pin_mut!(call_signaling_stream);

    loop {
        tokio::select! {
            opt = call_broadcast_stream.next() => {
                match opt {
                    Some(message) => {
                        if let Err(_e) = decode_broadcast_signal(message).await {
                            let _webrtc = webrtc.lock().await;
                            todo!("handle signal");
                        }
                    }
                    None => {
                        break
                    }
                }
            }
            opt = call_signaling_stream.next() => {
                match opt {
                    Some(_signal) => {
                        let _webrtc = webrtc.lock().await;
                        // todo: dial if needed
                        todo!("handle signal");
                    }
                    None => break
                }
            }
            opt = webrtc_event_stream.next() => {
                match opt {
                    Some(event) => {
                        log::debug!("webrtc event: {event}");
                        match event {
                            EmittedEvents::TrackAdded { peer, track } => {
                                if let Err(e) =   media_track::create_audio_sink_track(peer, track).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                            }
                            EmittedEvents::Disconnected { peer } => {
                                if let Err(e) = media_track::remove_sink_track(peer.clone()).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                                let mut s = webrtc.lock().await;
                                s.hang_up(&peer).await;
                            }
                            // todo: add audio devices when accepting a call
                            // todo: prompt the user that a call is incoming and let them accept it
                            EmittedEvents::CallInitiated { dest, sdp } => {
                                let mut s = webrtc.lock().await;
                                if let Err(e) = s.accept_call(&dest, *sdp).await {
                                    log::error!("failed to accept_call: {}", e);
                                    s.hang_up(&dest).await;
                                    // todo: is a disconnect signal needed here?
                                }
                            }
                            EmittedEvents::Sdp { dest, sdp } => {
                                let s = webrtc.lock().await;
                                if let Err(e) = s.recv_sdp(&dest, *sdp).await {
                                    log::error!("failed to recv_sdp: {}", e);
                                }
                            }
                            EmittedEvents::Ice { dest, candidate } => {
                                let s = webrtc.lock().await;
                                if let Err(e) = s.recv_ice(&dest, *candidate).await {
                                    log::error!("failed to recv_ice {}", e);
                                }
                            }
                        }
                        todo!("handle event");
                    }
                    None => todo!()
                }
            }
            _ = stop.notified() => {
                log::debug!("call termniated via notify()");
                break;
            }
        }
    }
}

// todo: move this elsewhere or delete it
/*pub mod media_track {
    use std::sync::Arc;

    use derive_more::Display;
    use warp::crypto::DID;
    use webrtc::{
        rtp_transceiver::rtp_codec::RTCRtpCodecCapability, track::track_remote::TrackRemote,
    };

    use crate::simple_webrtc::MediaSourceId;

    #[derive(Display)]
    pub enum Command {
        #[display(fmt = "CreateAudioSourceTrack")]
        CreateAudioSourceTrack { codec: RTCRtpCodecCapability },
        #[display(fmt = "RemoveSourceTrack")]
        RemoveSourceTrack { source_id: MediaSourceId },
        #[display(fmt = "CreateAudioSinkTrack")]
        CreateAudioSinkTrack {
            peer_id: DID,
            track: Arc<TrackRemote>,
        },
        #[display(fmt = "ChangeAudioOutput")]
        ChangeAudioOutput {
            host_id: cpal::HostId,
            device_name: String,
        },
        #[display(fmt = "ChangeAudioInput")]
        ChangeAudioInput {
            host_id: cpal::HostId,
            device_name: String,
        },
        #[display(fmt = "RemoveSinkTrack")]
        RemoveSinkTrack { peer_id: DID },
        #[display(fmt = "MutePeer")]
        MutePeer { peer_id: DID },
        #[display(fmt = "UnmutePeer")]
        UnmutePeer { peer_id: DID },
        #[display(fmt = "MuteSelf")]
        MuteSelf,
        #[display(fmt = "UnmuteSelf")]
        UnmuteSelf,
        #[display(fmt = "HangUp")]
        HangUp,
        #[display(fmt = "Quit")]
        Quit,
    }
}*/
