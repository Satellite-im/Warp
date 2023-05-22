//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!
//! IPFS subscriptions
//! - note that topics are automatically unsubscribed to when the stream is dropped
//! - call initiation: offer_call/<DID>
//! - per call
//!     - peer join/decline/leave call: telecon/<Uuid>
//!     - webrtc signaling: telecon/<Uuid>/<DID>
//!
//! Async Tasks
//! - One handles the offer_call topic
//!     - a broadcast::Sender<BlinkEventKind> is used to send events to the UI
//! - One to handle webrtc and related IPFS topics
//!     - uses the same broadcast::Sender<BlinkEventKind> to drive the UI
//!
//! WebRTC management
//! - the struct implementing Blink will keep a Arc<Mutex<simple_webrtc::Controller>>, allowing calls to be initiated by the UI
//! - the webrtc task also has that simple_webrtc::Controller

mod host_media;
mod signaling;
mod simple_webrtc;
mod store;

use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;
use once_cell::sync::Lazy;

use rust_ipfs::{Ipfs, SubscriptionStream};

use tokio::{
    sync::{
        broadcast::{self, Sender},
        Mutex,
    },
    task::JoinHandle,
};
use uuid::Uuid;
use warp::{
    blink::{self, Blink, BlinkEventKind, BlinkEventStream, CallInfo},
    crypto::{Fingerprint, DID},
    error::Error,
    multipass::MultiPass,
};

use crate::{
    signaling::{CallSignal, InitiationSignal, PeerSignal},
    simple_webrtc::events::{EmittedEvents, WebRtcEventStream},
    store::{
        decode_gossipsub_msg_aes, decode_gossipsub_msg_ecdh, send_signal_aes, send_signal_ecdh,
        PeerIdExt,
    },
};

#[derive(Clone)]
struct ActiveCall {
    call: CallInfo,
    connected_participants: HashSet<DID>,
}

// used when a call is accepted
impl From<CallInfo> for ActiveCall {
    fn from(value: CallInfo) -> Self {
        Self {
            call: value,
            connected_participants: HashSet::new(),
        }
    }
}

pub struct WebRtc {
    account: Box<dyn MultiPass>,
    ipfs: Ipfs,
    id: Arc<DID>,
    // a tx channel which emits events to drive the UI
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    // subscribes to IPFS topic to receive incoming calls
    offer_handler: JoinHandle<()>,
    // handles 3 streams: one for webrtc events and two IPFS topics
    // pertains to the active_call, which is stored in STATIC_DATA
    webrtc_handler: Option<JoinHandle<()>>,
}

static STATIC_DATA: Lazy<Mutex<StaticData>> = Lazy::new(|| {
    let (ui_event_ch, _rx) = broadcast::channel(1024);
    let webrtc = simple_webrtc::Controller::new().expect("failed to create webrtc controller");

    Mutex::new(StaticData {
        webrtc,
        ui_event_ch,
        cpal_host: cpal::default_host(),
        active_call: None,
        pending_calls: HashMap::new(),
    })
});

struct StaticData {
    webrtc: simple_webrtc::Controller,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    active_call: Option<ActiveCall>,
    pending_calls: HashMap<Uuid, CallInfo>,
    // todo: maybe get rid of this
    cpal_host: cpal::Host,
}

impl WebRtc {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Self> {
        log::trace!("initializing WebRTC");
        let _data = STATIC_DATA.lock().await;
        let identity = loop {
            if let Ok(identity) = account.get_own_identity().await {
                break identity;
            }
            tokio::time::sleep(Duration::from_millis(100)).await
        };
        let did = identity.did_key();

        let ipfs_handle = match account.handle() {
            Ok(handle) if handle.is::<Ipfs>() => handle.downcast_ref::<Ipfs>().cloned(),
            _ => anyhow::bail!("Unable to obtain IPFS Handle"),
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => {
                anyhow::bail!("Unable to use IPFS Handle");
            }
        };

        let cpal_host = cpal::platform::default_host();
        if let Some(d) = cpal_host.default_input_device() {
            host_media::change_audio_input(d).await?;
        }
        if let Some(d) = cpal_host.default_output_device() {
            host_media::change_audio_output(d).await?;
        }

        let call_offer_stream = match ipfs
            .pubsub_subscribe(ipfs_routes::call_initiation_route(&did))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to subscribe to call_broadcast_route: {e}");
                return Err(e);
            }
        };

        let (ui_event_ch, _rx) = broadcast::channel(1024);
        let ui_event_ch2 = ui_event_ch.clone();
        let own_id = Arc::new(account.decrypt_private_key(None)?);
        let own_id2 = own_id.clone();
        let offer_handler = tokio::spawn(async {
            handle_call_initiation(own_id2, call_offer_stream, ui_event_ch2).await;
        });

        let webrtc = Self {
            account,
            ipfs,
            id: own_id,
            ui_event_ch,
            offer_handler,
            webrtc_handler: None,
        };

        log::trace!("finished initializing WebRTC");
        Ok(webrtc)
    }

    async fn init_call(&mut self, data: &mut StaticData, call: CallInfo) -> anyhow::Result<()> {
        data.active_call.replace(call.clone().into());

        // next, create event streams and pass them to a task
        let call_broadcast_stream = self
            .ipfs
            .pubsub_subscribe(ipfs_routes::call_signal_route(&call.id()))
            .await
            .context("failed to subscribe to call_broadcast_route")?;

        let call_signaling_stream = self
            .ipfs
            .pubsub_subscribe(ipfs_routes::peer_signal_route(&self.id, &call.id()))
            .await
            .context("failed to subscribe to call_signaling_route")?;

        let webrtc_event_stream = WebRtcEventStream(Box::pin(
            data.webrtc
                .get_event_stream()
                .context("failed to get webrtc event stream")?,
        ));

        let ui_event_ch = self.ui_event_ch.clone();
        let own_id = self.id.clone();
        let ipfs2 = self.ipfs.clone();
        let webrtc_handle = tokio::task::spawn(async move {
            handle_webrtc(
                own_id,
                ipfs2,
                ui_event_ch,
                call_broadcast_stream,
                call_signaling_stream,
                webrtc_event_stream,
            )
            .await;
        });

        self.webrtc_handler.replace(webrtc_handle);
        Ok(())
    }

    async fn leave_call_internal(&mut self, data: &mut StaticData) -> anyhow::Result<()> {
        if let Some(ac) = data.active_call.take() {
            let call_id = ac.call.id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };

            if let Err(e) = send_signal_aes(&self.ipfs, &ac.call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            }

            let r = data.webrtc.deinit().await;
            host_media::reset().await;
            r
        } else {
            Ok(())
        }
    }
}

impl Drop for WebRtc {
    fn drop(&mut self) {
        self.offer_handler.abort();
    }
}

async fn handle_call_initiation(
    own_id: Arc<DID>,
    mut stream: SubscriptionStream,
    ch: Sender<BlinkEventKind>,
) {
    while let Some(msg) = stream.next().await {
        let sender = match msg.source.and_then(|s| s.to_did().ok()) {
            Some(id) => id,
            None => {
                log::error!("msg received without source");
                continue;
            }
        };

        let signal: InitiationSignal = match decode_gossipsub_msg_ecdh(&own_id, &sender, &msg) {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to decode msg from call initiation stream: {e}");
                continue;
            }
        };

        match signal {
            InitiationSignal::Offer { call_info } => {
                let evt = BlinkEventKind::IncomingCall {
                    call_id: call_info.id(),
                    sender,
                    participants: call_info.participants(),
                };

                let mut data = STATIC_DATA.lock().await;
                data.pending_calls.insert(call_info.id(), call_info);
                if let Err(e) = ch.send(evt) {
                    log::error!("failed to send IncomingCall Event: {e}");
                }
            }
        }
    }
}

async fn handle_webrtc(
    own_id: Arc<DID>,
    ipfs: Ipfs,
    ch: Sender<BlinkEventKind>,
    call_signaling_stream: SubscriptionStream,
    peer_signaling_stream: SubscriptionStream,
    mut webrtc_event_stream: WebRtcEventStream,
) {
    futures::pin_mut!(call_signaling_stream);
    futures::pin_mut!(peer_signaling_stream);

    loop {
        tokio::select! {
            opt = call_signaling_stream.next() => {
                let msg = match opt {
                    Some(m) => m,
                    None => continue
                };
                let sender = match msg.source.and_then(|s| s.to_did().ok()) {
                    Some(id) => id,
                    None => {
                        log::error!("msg received without source");
                        continue
                    }
                };
                let mut data = STATIC_DATA.lock().await;
                let ac = match data.active_call.as_ref() {
                    Some(r) => r,
                    None => {
                        log::error!("received call signal without an active call");
                        continue;
                    }
                };
                let signal: CallSignal = match decode_gossipsub_msg_aes(&ac.call.group_key(), &msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };
                match signal {
                    CallSignal::Join { call_id } => {
                        if let Some(ac) = data.active_call.as_mut() {
                            ac.connected_participants.insert(sender.clone());
                        }
                        // emits CallInitiated Event, which returns the local sdp. will be sent to the peer with the dial signal
                        data.webrtc.dial(&sender).await;
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantJoined { call_id, peer_id: sender }) {
                            log::error!("failed to send ParticipantJoined Event: {e}");
                        }

                    }
                    CallSignal::Leave { call_id } => {
                        if let Some(ac) = data.active_call.as_mut() {
                            ac.connected_participants.remove(&sender);
                        }
                        data.webrtc.hang_up(&sender).await;
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantLeft { call_id, peer_id: sender }) {
                            log::error!("failed to send ParticipantLeft Event: {e}");
                        }
                    },
                }
            },
            opt = peer_signaling_stream.next() => {
                let msg = match opt {
                    Some(m) => m,
                    None => continue
                };
                let sender = match msg.source.and_then(|s| s.to_did().ok()) {
                    Some(id) => id,
                    None => {
                        log::error!("msg received without source");
                        continue
                    }
                };
                let signal: PeerSignal = match decode_gossipsub_msg_ecdh(&own_id, &sender, &msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };
                let mut data = STATIC_DATA.lock().await;
                let ac = match data.active_call.as_ref() {
                    Some(r) => r,
                    None => {
                        log::error!("received a peer_signal when there is no active call");
                        continue;
                    }
                };
                if !ac.call.participants().contains(&sender) {
                    log::error!("received a signal from a peer who isn't part of the call");
                    continue;
                }

                match signal {
                    PeerSignal::Ice(ice) => {
                        if let Err(e) = data.webrtc.recv_ice(&sender, ice).await {
                            log::error!("failed to recv_ice {}", e);
                        }
                    }
                    PeerSignal::Sdp(sdp) => {
                        if let Err(e) = data.webrtc.recv_sdp(&sender, sdp).await {
                            log::error!("failed to recv_sdp: {}", e);
                        }
                    }
                    PeerSignal::Dial(sdp) => {
                        if !host_media::has_audio_source().await {
                            let audio_codec = ac.call.audio_codec();
                            let codec = RTCRtpCodecCapability {
                                mime_type: audio_codec.mime_type(),
                                clock_rate: audio_codec.sample_rate(),
                                channels: audio_codec.channels(),
                                ..Default::default()
                            };
                            let track = match data
                                .webrtc
                                .add_media_source(host_media::AUDIO_SOURCE_ID.into(), codec.clone())
                                .await {
                                    Ok(r) => r,
                                    Err(e) => {
                                        log::error!("failed to add media source: {e}");
                                        continue;
                                    }
                                };
                            if let Err(e) = host_media::create_audio_source_track(track, audio_codec).await {
                                log::error!("failed to create audio source track: {e}");
                                data.webrtc.hang_up(&sender).await;
                                // todo: how to leave the call...may need to send a signal
                                todo!()
                            }
                        }

                        // emits the SDP Event, which is sent to the peer via the SDP signal
                        if let Err(e) = data.webrtc.accept_call(&sender, sdp).await {
                            log::error!("failed to accept_call: {}", e);
                        }

                    }
                }
            },
            opt = webrtc_event_stream.next() => {
                let mut data = STATIC_DATA.lock().await;
                match opt {
                    Some(event) => {
                        log::debug!("webrtc event: {event}");
                        let active_call = match data.active_call.as_ref() {
                            Some(ac) => ac,
                            None => {
                                log::error!("event emitted but no active call");
                                continue;
                            }
                        };
                        let call_id = active_call.call.id();
                        match event {
                            EmittedEvents::TrackAdded { peer, track } => {
                                if let Err(e) =   host_media::create_audio_sink_track(peer.clone(), track).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                            }
                            EmittedEvents::Disconnected { peer } => {
                                if let Err(e) = host_media::remove_sink_track(peer.clone()).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                                data.webrtc.hang_up(&peer).await;
                            }
                            EmittedEvents::CallInitiated { dest, sdp } => {
                                let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                                let signal = PeerSignal::Dial(*sdp);
                                if let Err(e) = send_signal_ecdh(&ipfs, &own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Sdp { dest, sdp } => {
                                let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                                let signal = PeerSignal::Sdp(*sdp);
                                if let Err(e) = send_signal_ecdh(&ipfs, &own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Ice { dest, candidate } => {
                               let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                               let signal = PeerSignal::Ice(*candidate);
                                if let Err(e) = send_signal_ecdh(&ipfs, &own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                        }
                    }
                    None => todo!()
                }
            }
        }
    }
}

mod ipfs_routes {
    use uuid::Uuid;
    use warp::crypto::DID;

    const TELECON_BROADCAST: &str = "telecon";
    const OFFER_CALL: &str = "offer_call";

    /// subscribe/unsubscribe per-call
    /// CallSignal
    pub fn call_signal_route(call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}")
    }

    /// subscribe/unsubscribe per-call
    /// PeerSignal
    pub fn peer_signal_route(peer: &DID, call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}/{peer}")
    }

    /// subscribe to this when initializing Blink
    /// InitiationSignal
    pub fn call_initiation_route(peer: &DID) -> String {
        format!("{OFFER_CALL}/{peer}")
    }
}

/// blink implementation
///
///
#[async_trait]
impl Blink for WebRtc {
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
    async fn offer_call(
        &mut self,
        mut participants: Vec<DID>,
        audio_codec: blink::AudioCodec,
        video_codec: blink::VideoCodec,
        _screenshare_codec: blink::VideoCodec,
    ) -> Result<(), Error> {
        if !participants.contains(&self.id) {
            participants.push(DID::from_str(&self.id.fingerprint())?);
        };
        let mut data = STATIC_DATA.lock().await;
        let call_info = CallInfo::new(participants.clone(), audio_codec.clone(), video_codec);
        let ac = ActiveCall::from(call_info.clone());

        self.leave_call_internal(&mut data).await?;

        // ensure there is an audio source track
        let _audio_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
            mime_type: audio_codec.mime_type(),
            clock_rate: audio_codec.sample_rate(),
            channels: audio_codec.channels(),
            ..Default::default()
        };
        let track = data
            .webrtc
            .add_media_source(host_media::AUDIO_SOURCE_ID.into(), _audio_codec.clone())
            .await?;
        host_media::create_audio_source_track(track, audio_codec).await?;

        self.init_call(&mut data, call_info.clone()).await?;
        for dest in participants {
            if dest == *self.id {
                continue;
            }
            let topic = ipfs_routes::call_initiation_route(&dest);
            let signal = InitiationSignal::Offer {
                call_info: ac.call.clone(),
            };

            if let Err(e) = send_signal_ecdh(&self.ipfs, &self.id, &dest, signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
        }
        Ok(())
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        self.leave_call_internal(&mut data).await?;
        if let Some(call) = data.pending_calls.remove(&call_id) {
            self.init_call(&mut data, call.clone()).await?;
            let call_id = call.id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Join { call_id };
            if let Err(e) = send_signal_aes(&self.ipfs, &call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
        }
        Ok(())
    }
    /// use the Leave signal as a courtesy, to let the group know not to expect you to join.
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        if let Some(call) = data.pending_calls.remove(&call_id) {
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };
            if let Err(e) = send_signal_aes(&self.ipfs, &call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
        }
        Ok(())
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        // todo: host_media::remove_source_track(host_media::AUDIO_SOURCE_ID).await?;
        self.leave_call_internal(&mut data).await?;
        Ok(())
    }

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error> {
        let data = STATIC_DATA.lock().await;
        let device_iter = match data.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn get_current_microphone(&self) -> Option<String> {
        host_media::get_input_device_name().await
    }
    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error> {
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
                    host_media::change_audio_input(device).await?;
                    return Ok(());
                }
            }
        }

        Err(warp::error::Error::OtherWithContext(
            "input device not found".into(),
        ))
    }
    async fn select_default_microphone(&mut self) -> Result<(), Error> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(Error::OtherWithContext(String::from(
                "no default input device",
            )))?;
        host_media::change_audio_input(device).await?;
        Ok(())
    }
    async fn get_available_speakers(&self) -> Result<Vec<String>, Error> {
        let data = STATIC_DATA.lock().await;
        let device_iter = match data.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn get_current_speaker(&self) -> Option<String> {
        host_media::get_output_device_name().await
    }
    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error> {
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
                    host_media::change_audio_output(device).await?;
                    return Ok(());
                }
            }
        }

        Err(warp::error::Error::OtherWithContext(
            "output device not found".into(),
        ))
    }
    async fn select_default_speaker(&mut self) -> Result<(), Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(Error::OtherWithContext(String::from(
                "no default input device",
            )))?;
        host_media::change_audio_output(device).await?;
        Ok(())
    }
    async fn get_available_cameras(&self) -> Result<Vec<String>, Error> {
        todo!()
    }
    async fn select_camera(&mut self, _device_name: &str) -> Result<(), Error> {
        todo!()
    }

    async fn select_default_camera(&mut self) -> Result<(), Error> {
        todo!()
    }

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error> {
        host_media::mute_self()
            .await
            .map_err(|e| warp::error::Error::OtherWithContext(e.to_string()))
    }
    async fn unmute_self(&mut self) -> Result<(), Error> {
        host_media::unmute_self()
            .await
            .map_err(|e| warp::error::Error::OtherWithContext(e.to_string()))
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

    async fn pending_calls(&self) -> Vec<CallInfo> {
        let data = STATIC_DATA.lock().await;
        Vec::from_iter(data.pending_calls.values().cloned())
    }
    async fn current_call(&self) -> Option<CallInfo> {
        let data = STATIC_DATA.lock().await;
        data.active_call.as_ref().map(|x| x.call.clone())
    }
}
