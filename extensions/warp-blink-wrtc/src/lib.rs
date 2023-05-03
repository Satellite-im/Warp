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
    // manages media tracks
    media_track_ch: mpsc::UnboundedSender<media_track::Command>,
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

        self.media_track_ch
            .send(media_track::Command::ChangeAudioInput {
                host_id: self.cpal_host.id(),
                device_name: device_name.into(),
            })
            .map_err(|e| {
                warp::error::Error::OtherWithContext(format!("failed to change input device: {e}"))
            })?;
        self.audio_input = new_input;

        Ok(())
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

        self.media_track_ch
            .send(media_track::Command::ChangeAudioOutput {
                host_id: self.cpal_host.id(),
                device_name: device_name.into(),
            })
            .map_err(|e| {
                warp::error::Error::OtherWithContext(format!("failed to change output device: {e}"))
            })?;
        self.audio_output = new_output;

        Ok(())
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
        let (media_track_ch, media_rx) = mpsc::unbounded_channel();

        let webrtc2 = webrtc.clone();
        std::thread::spawn(|| {
            if let Err(_e) = manage_streams(webrtc2, media_rx) {
                todo!();
            }
        });

        let cpal_host = cpal::platform::default_host();
        let input_device = cpal_host
            .default_input_device()
            .and_then(|device| device.name().ok());
        let output_device = cpal_host
            .default_output_device()
            .and_then(|device| device.name().ok());

        let webrtc = Self {
            webrtc,
            account,
            ipfs: Arc::new(RwLock::new(ipfs.clone())),
            id: did.clone(),
            ui_event_ch,
            media_track_ch,
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

        // warning: a media source must be added before attempting to connect or SDP will fail
        if let Some(device_name) = self.audio_input.clone() {
            // todo: let the user pick the codec
            let codec = RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 48000,
                channels: opus::Channels::Mono as u16,
                ..Default::default()
            };
            if let Err(e) = self
                .media_track_ch
                .send(media_track::Command::ChangeAudioInput {
                    host_id: self.cpal_host.id(),
                    device_name,
                })
            {
                log::error!("failed to send ChangeAudioInput command: {e}");
            }
            if let Err(e) = self
                .media_track_ch
                .send(media_track::Command::CreateAudioSourceTrack { codec })
            {
                log::error!("failed to send CreateSourceTrack command: {e}");
            }
        }

        if let Some(device_name) = self.audio_output.clone() {
            if let Err(e) = self
                .media_track_ch
                .send(media_track::Command::ChangeAudioOutput {
                    host_id: self.cpal_host.id(),
                    device_name,
                })
            {
                log::error!("failed to send ChangeAudioOutput command: {e}");
            }
        }

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

fn manage_streams(
    webrtc: Arc<Mutex<Controller>>,
    mut ch: mpsc::UnboundedReceiver<media_track::Command>,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new()?;
    let mut audio_input_device: Option<cpal::Device> = None;
    let mut audio_output_device: Option<cpal::Device> = None;
    let mut audio_source: Option<Box<dyn audio::SourceTrack>> = None;
    let mut sink_tracks: HashMap<Uuid, Box<dyn audio::SinkTrack>> = HashMap::new();

    let audio_source_id = String::from("audio-input");
    while let Some(cmd) = ch.blocking_recv() {
        match cmd {
            media_track::Command::CreateAudioSourceTrack { codec } => {
                if audio_source.is_some() {
                    rt.block_on(async {
                        let mut s = webrtc.lock().await;
                        if let Err(e) = s.remove_media_source(audio_source_id.clone()).await {
                            log::error!("failed to remove audio source: {e}");
                        }
                    });
                }

                let input_device = match audio_input_device.as_ref() {
                    Some(d) => d,
                    None => {
                        log::error!("no audio input device selected");
                        continue;
                    }
                };

                let source_track = rt.block_on(async {
                    create_source_track(&webrtc, input_device, codec, audio_source_id.clone()).await
                })?;
                source_track.play()?;
                audio_source.replace(source_track);
            }
            media_track::Command::CreateAudioSinkTrack { track } => {
                let codec = rt.block_on(async { track.codec().await.capability });
                let output_device = match audio_output_device.as_ref() {
                    Some(d) => d,
                    None => {
                        log::error!("no audio output device selected");
                        continue;
                    }
                };

                let sink_track =
                    simple_webrtc::audio::create_sink_track(output_device, track, codec)?;
                sink_track.play()?;
                sink_tracks.insert(sink_track.id(), sink_track);
            }
            media_track::Command::ChangeAudioInput {
                host_id,
                device_name,
            } => {
                let device = match simple_webrtc::audio::get_input_device(host_id, device_name) {
                    Ok(d) => d,
                    Err(e) => {
                        log::error!("failed to get input device: {e}");
                        continue;
                    }
                };
                if let Some(source) = audio_source.as_mut() {
                    source.change_input_device(&device);
                } else {
                    log::error!("no audio input to change");
                }

                audio_input_device.replace(device);
            }
            media_track::Command::ChangeAudioOutput {
                host_id,
                device_name,
            } => {
                let device =
                    match simple_webrtc::audio::get_output_device(host_id, device_name.clone()) {
                        Ok(d) => d,
                        Err(e) => {
                            log::error!("failed to get output device: {e}");
                            continue;
                        }
                    };
                for (_k, v) in sink_tracks.iter_mut() {
                    if let Err(e) = v.change_output_device(&device) {
                        log::error!("failed to change output device: {e}");
                    }
                }

                audio_output_device.replace(device);
            }
            _ => todo!(),
        }
    }

    Ok(())
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
                    Some(_event) => {
                        let _webrtc = webrtc.lock().await;
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
pub mod media_track {
    use std::sync::Arc;

    use webrtc::{
        rtp_transceiver::rtp_codec::RTCRtpCodecCapability, track::track_remote::TrackRemote,
    };

    use crate::simple_webrtc::MediaSourceId;

    pub enum Command {
        CreateAudioSourceTrack {
            codec: RTCRtpCodecCapability,
        },
        RemoveSourceTrack {
            source_id: MediaSourceId,
        },
        CreateAudioSinkTrack {
            track: Arc<TrackRemote>,
        },
        ChangeAudioOutput {
            host_id: cpal::HostId,
            device_name: String,
        },
        ChangeAudioInput {
            host_id: cpal::HostId,
            device_name: String,
        },
        RemoveSinkTrack,
        Reset,
    }
}
