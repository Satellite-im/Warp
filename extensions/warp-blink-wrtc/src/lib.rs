//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!
//! The init() function must be called prior to using the Blink implementation.
//! the deinit() function must be called to ensure all threads are cleaned up properly.
//!
//! init() returns a BlinkImpl struct, which as the name suggests, implements Blink.
//! All data used by the implementation is contained in two static variables: IPFS and BLINK_DATA.
//!

// #![allow(dead_code)]

mod host_media;
mod signaling;
mod simple_webrtc;
mod store;

use async_trait::async_trait;
use std::{any::Any, collections::HashMap, str::FromStr, sync::Arc, time::Duration};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

use anyhow::{bail, Context};
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;

use rust_ipfs::{Ipfs, SubscriptionStream};

use tokio::{
    sync::{
        broadcast::{self, Sender},
        RwLock,
    },
    task::JoinHandle,
};
use uuid::Uuid;
use warp::{
    blink::{
        self, AudioCodec, AudioProcessingConfig, Blink, BlinkEventKind, BlinkEventStream, CallInfo,
        MimeType,
    },
    crypto::{Fingerprint, DID},
    error::Error,
    module::Module,
    multipass::MultiPass,
    Extension, SingleHandle,
};

use crate::{
    signaling::{ipfs_routes, CallSignal, InitiationSignal, PeerSignal},
    simple_webrtc::events::{EmittedEvents, WebRtcEventStream},
    store::{
        decode_gossipsub_msg_aes, decode_gossipsub_msg_ecdh, send_signal_aes, send_signal_ecdh,
        PeerIdExt,
    },
};

// implements Blink
#[derive(Clone)]
pub struct BlinkImpl {
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    pending_calls: Arc<RwLock<HashMap<Uuid, CallInfo>>>,
    active_call: Arc<RwLock<Option<ActiveCall>>>,
    webrtc_controller: Arc<RwLock<simple_webrtc::Controller>>,
    // the DID generated from Multipass, never cloned. contains the private key
    own_id: Arc<RwLock<Option<DID>>>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    audio_source_codec: Arc<RwLock<blink::AudioCodec>>,
    audio_sink_codec: Arc<RwLock<blink::AudioCodec>>,

    // subscribes to IPFS topic to receive incoming calls
    offer_handler: Arc<warp::sync::RwLock<JoinHandle<()>>>,
    // handles 3 streams: one for webrtc events and two IPFS topics
    // pertains to the active_call, which is stored in STATIC_DATA
    webrtc_handler: Arc<warp::sync::RwLock<Option<JoinHandle<()>>>>,
}

#[derive(Clone)]
struct ActiveCall {
    call: CallInfo,
    connected_participants: HashMap<DID, PeerState>,
    call_state: CallState,
}

#[derive(Clone, Eq, PartialEq)]
pub enum PeerState {
    Disconnected,
    Initializing,
    Connected,
    Closed,
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CallState {
    // the call was offered but no one joined and there is no peer connection
    Uninitialized,
    // at least one peer has connected
    Started,
    Closing,
    Closed,
}

// used when a call is accepted
impl From<CallInfo> for ActiveCall {
    fn from(value: CallInfo) -> Self {
        Self {
            call: value,
            connected_participants: HashMap::new(),
            call_state: CallState::Uninitialized,
        }
    }
}

impl Drop for BlinkImpl {
    fn drop(&mut self) {
        let webrtc_handler = std::mem::take(&mut *self.webrtc_handler.write());
        if let Some(handle) = webrtc_handler {
            handle.abort();
        }
        self.offer_handler.write().abort();
        let webrtc_controller = self.webrtc_controller.clone();
        tokio::spawn(async move {
            let _ = webrtc_controller.write().await.deinit().await;
            log::debug!("deinit finished");
        });
    }
}

impl BlinkImpl {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Box<Self>> {
        log::trace!("initializing WebRTC");

        let cpal_host = cpal::platform::default_host();
        if let Some(d) = cpal_host.default_input_device() {
            host_media::change_audio_input(d).await?;
        }
        if let Some(d) = cpal_host.default_output_device() {
            host_media::change_audio_output(d).await?;
        }

        // check SupportedStreamConfigs. if those channels aren't supported, use the default.
        let mut source_codec = blink::AudioCodec {
            mime: MimeType::OPUS,
            sample_rate: blink::AudioSampleRate::High,
            channels: 1,
        };

        let mut sink_codec = blink::AudioCodec {
            mime: MimeType::OPUS,
            sample_rate: blink::AudioSampleRate::High,
            channels: 1,
        };

        let cpal_host = cpal::default_host();
        if let Some(input_device) = cpal_host.default_input_device() {
            if let Ok(mut configs) = input_device.supported_input_configs() {
                if !configs.any(|c| c.channels() == source_codec.channels()) {
                    if let Ok(default_config) = input_device.default_input_config() {
                        source_codec.channels = default_config.channels();
                    }
                }
            }
        }

        if let Some(output_device) = cpal_host.default_output_device() {
            if let Ok(mut configs) = output_device.supported_output_configs() {
                if !configs.any(|c| c.channels() == sink_codec.channels()) {
                    if let Ok(default_config) = output_device.default_output_config() {
                        sink_codec.channels = default_config.channels();
                    }
                }
            }
        }

        let (ui_event_ch, _rx) = broadcast::channel(1024);
        let blink_impl = Self {
            ipfs: Arc::new(RwLock::new(None)),
            pending_calls: Arc::new(RwLock::new(HashMap::new())),
            active_call: Arc::new(RwLock::new(None)),
            webrtc_controller: Arc::new(RwLock::new(simple_webrtc::Controller::new()?)),
            own_id: Arc::new(RwLock::new(None)),
            ui_event_ch,
            audio_source_codec: Arc::new(RwLock::new(source_codec)),
            audio_sink_codec: Arc::new(RwLock::new(sink_codec)),
            offer_handler: Arc::new(warp::sync::RwLock::new(tokio::spawn(async {}))),
            webrtc_handler: Arc::new(warp::sync::RwLock::new(None)),
        };

        let ipfs = blink_impl.ipfs.clone();
        let own_id = blink_impl.own_id.clone();
        let offer_handler = blink_impl.offer_handler.clone();
        let pending_calls = blink_impl.pending_calls.clone();
        let ui_event_ch = blink_impl.ui_event_ch.clone();

        tokio::spawn(async move {
            let f = async {
                //Note: This will block the initialization until identity is created or loaded
                //TODO: Push into separate task if it initially fails
                let identity = loop {
                    if let Ok(identity) = account.get_own_identity().await {
                        break identity;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await
                };
                let ipfs_handle = match account.handle() {
                    Ok(handle) if handle.is::<Ipfs>() => handle.downcast_ref::<Ipfs>().cloned(),
                    _ => {
                        bail!("Unable to obtain IPFS Handle")
                    }
                };

                let _ipfs = match ipfs_handle {
                    Some(r) => r,
                    None => bail!("Unable to use IPFS Handle"),
                };

                let call_offer_stream = match _ipfs
                    .pubsub_subscribe(ipfs_routes::call_initiation_route(&identity.did_key()))
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to subscribe to call_broadcast_route: {e}");
                        return Err(e);
                    }
                };

                let _own_id = account.decrypt_private_key(None)?;
                own_id.write().await.replace(_own_id);

                let own_id2 = own_id.clone();
                let _offer_handler = tokio::spawn(async {
                    handle_call_initiation(own_id2, pending_calls, call_offer_stream, ui_event_ch)
                        .await;
                });

                let mut x = offer_handler.write();
                *x = _offer_handler;

                // set ipfs last to quickly detect that webrtc hasn't been initialized.
                ipfs.write().await.replace(_ipfs);
                log::trace!("finished initializing WebRTC");
                Ok(())
            };

            // todo: put this in a loop?
            if let Err(e) = f.await {
                log::error!("failed to init blink: {e}");
            }
        });

        Ok(Box::new(blink_impl))
    }

    async fn init_call(&mut self, call: CallInfo) -> anyhow::Result<()> {
        let lock = self.ipfs.read().await;
        let ipfs = match lock.as_ref() {
            Some(r) => r,
            None => bail!("blink not initialized"),
        };
        let lock = self.own_id.read().await;
        let own_id = match lock.as_ref() {
            Some(r) => r,
            None => bail!("blink not initialized"),
        };
        self.active_call.write().await.replace(call.clone().into());
        let audio_source_codec = self.audio_source_codec.read().await;
        // ensure there is an audio source track
        let rtc_rtp_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
            mime_type: audio_source_codec.mime_type(),
            clock_rate: audio_source_codec.sample_rate(),
            channels: audio_source_codec.channels(),
            ..Default::default()
        };
        let track = self
            .webrtc_controller
            .write()
            .await
            .add_media_source(host_media::AUDIO_SOURCE_ID.into(), rtc_rtp_codec)
            .await?;
        host_media::create_audio_source_track(
            self.ui_event_ch.clone(),
            track,
            call.codec(),
            audio_source_codec.clone(),
        )
        .await?;

        // next, create event streams and pass them to a task
        let call_signaling_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_signal_route(&call.call_id()))
            .await
            .context("failed to subscribe to call_broadcast_route")?;

        let peer_signaling_stream = ipfs
            .pubsub_subscribe(ipfs_routes::peer_signal_route(own_id, &call.call_id()))
            .await
            .context("failed to subscribe to call_signaling_route")?;

        let webrtc_event_stream = WebRtcEventStream(Box::pin(
            self.webrtc_controller
                .read()
                .await
                .get_event_stream()
                .context("failed to get webrtc event stream")?,
        ));

        let webrtc_handler = std::mem::take(&mut *self.webrtc_handler.write());
        if let Some(handle) = webrtc_handler {
            // just to be safe
            handle.abort();
        }

        let own_id = self.own_id.clone();
        let ipfs2 = self.ipfs.clone();
        let active_call = self.active_call.clone();
        let webrtc_controller = self.webrtc_controller.clone();
        let pending_calls = self.pending_calls.clone();
        let audio_sink_codec = self.audio_sink_codec.clone();
        let ui_event_ch = self.ui_event_ch.clone();
        let event_ch2 = ui_event_ch.clone();

        let webrtc_handle = tokio::task::spawn(async move {
            handle_webrtc(
                WebRtcHandlerParams {
                    own_id,
                    event_ch: event_ch2,
                    ipfs: ipfs2,
                    active_call,
                    webrtc_controller,
                    _pending_calls: pending_calls,
                    audio_sink_codec,
                    ch: ui_event_ch,
                    call_signaling_stream,
                    peer_signaling_stream,
                },
                webrtc_event_stream,
            )
            .await;
        });

        self.webrtc_handler.write().replace(webrtc_handle);
        Ok(())
    }
}

async fn handle_call_initiation(
    own_id: Arc<RwLock<Option<DID>>>,
    pending_calls: Arc<RwLock<HashMap<Uuid, CallInfo>>>,
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

        let lock = own_id.read().await;
        let own_id = match lock.as_ref() {
            Some(r) => r,
            None => {
                log::error!("own_id not initialized");
                continue;
            }
        };
        let signal: InitiationSignal = match decode_gossipsub_msg_ecdh(own_id, &sender, &msg) {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to decode msg from call initiation stream: {e}");
                continue;
            }
        };

        match signal {
            InitiationSignal::Offer { call_info } => {
                let evt = BlinkEventKind::IncomingCall {
                    call_id: call_info.call_id(),
                    conversation_id: call_info.conversation_id(),
                    sender,
                    participants: call_info.participants(),
                };

                pending_calls
                    .write()
                    .await
                    .insert(call_info.call_id(), call_info);
                if let Err(e) = ch.send(evt) {
                    log::error!("failed to send IncomingCall Event: {e}");
                }
            }
        }
    }
}

struct WebRtcHandlerParams {
    own_id: Arc<RwLock<Option<DID>>>,
    event_ch: broadcast::Sender<BlinkEventKind>,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    active_call: Arc<RwLock<Option<ActiveCall>>>,
    webrtc_controller: Arc<RwLock<simple_webrtc::Controller>>,
    _pending_calls: Arc<RwLock<HashMap<Uuid, CallInfo>>>,
    audio_sink_codec: Arc<RwLock<blink::AudioCodec>>,
    ch: Sender<BlinkEventKind>,
    call_signaling_stream: SubscriptionStream,
    peer_signaling_stream: SubscriptionStream,
}

async fn handle_webrtc(params: WebRtcHandlerParams, mut webrtc_event_stream: WebRtcEventStream) {
    let WebRtcHandlerParams {
        own_id,
        event_ch,
        ipfs,
        active_call,
        webrtc_controller,
        _pending_calls,
        audio_sink_codec,
        ch,
        call_signaling_stream,
        peer_signaling_stream,
    } = params;
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
                let mut lock = active_call.write().await;
                let active_call = match lock.as_mut() {
                    Some(r) => r,
                    None => {
                        log::error!("received call signal without an active call");
                        continue;
                    }
                };
                let signal: CallSignal = match decode_gossipsub_msg_aes(&active_call.call.group_key(), &msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };
                match signal {
                    CallSignal::Join { call_id } => {
                        log::debug!("received signal: Join");
                        match active_call.call_state.clone() {
                            CallState::Uninitialized => active_call.call_state = CallState::Started,
                            x => if x != CallState::Started {
                                     log::error!("someone tried to join call with state: {:?}", active_call.call_state);
                                    continue;
                            }
                        }
                        active_call.connected_participants.insert(sender.clone(), PeerState::Initializing);
                        // todo: properly hang up on error.
                        // emits CallInitiated Event, which returns the local sdp. will be sent to the peer with the dial signal
                        if let Err(e) = webrtc_controller.write().await.dial(&sender).await {
                            log::error!("failed to dial peer: {e}");
                            continue;
                        }
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantJoined { call_id, peer_id: sender }) {
                            log::error!("failed to send ParticipantJoined Event: {e}");
                        }

                    }
                    CallSignal::Leave { call_id } => {
                        log::debug!("received signal: Leave");
                        if active_call.call_state == CallState::Closed {
                            log::error!("participant tried to leave a call which was already closed");
                            continue;
                        }
                        if active_call.call.call_id() != call_id {
                            log::error!("participant tried to leave call which wasn't active");
                            continue;
                        }
                        if !active_call.call.participants().contains(&sender) {
                            log::error!("participant tried to leave call who wasn't part of the call");
                            continue;
                        }
                        webrtc_controller.write().await.hang_up(&sender).await;
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
                let lock = own_id.read().await;
                let own_id = match lock.as_ref() {
                    Some(r) => r,
                    None => {
                        log::debug!("received signal before blink is initialized");
                        continue;
                    }
                };
                let sender = match msg.source.and_then(|s| s.to_did().ok()) {
                    Some(id) => id,
                    None => {
                        log::error!("msg received without source");
                        continue
                    }
                };
                let signal: PeerSignal = match decode_gossipsub_msg_ecdh(own_id, &sender, &msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };
                let mut lock = active_call.write().await;
                let active_call = match lock.as_mut() {
                    Some(r) => r,
                    None => {
                        log::error!("received a peer_signal when there is no active call");
                        continue;
                    }
                };
                if matches!(active_call.call_state, CallState::Closing | CallState::Closed) {
                    log::warn!("received a signal for a call which is being closed");
                    continue;
                }
                if !active_call.call.participants().contains(&sender) {
                    log::error!("received a signal from a peer who isn't part of the call");
                    continue;
                }

                let mut webrtc_controller = webrtc_controller.write().await;

                match signal {
                    PeerSignal::Ice(ice) => {
                        if active_call.call_state != CallState::Started {
                            log::error!("ice received for uninitialized call");
                            continue;
                        }
                        if let Err(e) = webrtc_controller.recv_ice(&sender, ice).await {
                            log::error!("failed to recv_ice {}", e);
                        }
                    }
                    PeerSignal::Sdp(sdp) => {
                        if active_call.call_state != CallState::Started {
                            log::error!("sdp received for uninitialized call");
                            continue;
                        }
                        log::debug!("received signal: SDP");
                        if let Err(e) = webrtc_controller.recv_sdp(&sender, sdp).await {
                            log::error!("failed to recv_sdp: {}", e);
                        }
                    }
                    PeerSignal::Dial(sdp) => {
                        if active_call.call_state == CallState::Uninitialized {
                            active_call.call_state = CallState::Started;
                        }
                        log::debug!("received signal: Dial");
                        // emits the SDP Event, which is sent to the peer via the SDP signal
                        if let Err(e) = webrtc_controller.accept_call(&sender, sdp).await {
                            log::error!("failed to accept_call: {}", e);
                        }
                    }
                }
            },
            opt = webrtc_event_stream.next() => {
                match opt {
                    Some(event) => {
                        if let EmittedEvents::Ice{ .. } = event {
                            // don't log this event. it is too noisy.
                            // would use matches! but the enum's fields don't implement PartialEq
                        } else {
                            log::debug!("webrtc event: {event}");
                        }
                        let lock = own_id.read().await;
                        let own_id = match lock.as_ref() {
                            Some(r) => r,
                            None => {
                                log::debug!("received signal before blink is initialized");
                                continue;
                            }
                        };
                        let lock = ipfs.read().await;
                        let ipfs = match lock.as_ref() {
                            Some(r) => r,
                            None => {
                                log::debug!("received signal before blink is initialized");
                                continue;
                            }
                        };
                        let mut lock = active_call.write().await;
                        let active_call = match lock.as_mut() {
                            Some(ac) => ac,
                            None => {
                                log::error!("event emitted but no active call");
                                continue;
                            }
                        };
                        let mut webrtc_controller = webrtc_controller.write().await;
                        let call_id = active_call.call.call_id();
                        match event {
                            EmittedEvents::TrackAdded { peer, track } => {
                                let webrtc_codec = active_call.call.codec();
                                if peer == *own_id {
                                    log::warn!("got TrackAdded event for own id");
                                    continue;
                                }
                                let audio_sink_codec = audio_sink_codec.read().await.clone();
                                if let Err(e) =   host_media::create_audio_sink_track(peer.clone(), event_ch.clone(), track, webrtc_codec, audio_sink_codec).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                            }
                            EmittedEvents::Connected { peer } => {
                                active_call.connected_participants.insert(peer.clone(), PeerState::Connected);
                                let event = BlinkEventKind::ParticipantJoined { call_id, peer_id: peer};
                                let _ = ch.send(event);
                            }
                            EmittedEvents::ConnectionClosed { peer } => {
                                // sometimes this event triggers without Disconnected being triggered.
                                // need to hang_up here as well.
                                active_call.connected_participants.insert(peer.clone(), PeerState::Closed);
                                let all_closed = !active_call.connected_participants.iter().any(|(_k, v)| *v != PeerState::Closed);
                                if all_closed {
                                    active_call.call_state = CallState::Closed;
                                }
                                // have to use data after active_call or there will be 2 mutable borrows, which isn't allowed
                                webrtc_controller.hang_up(&peer).await;
                                // this log should appear after the logs emitted by hang_up
                                if all_closed {
                                    log::info!("all participants have successfully been disconnected");
                                    if let Err(e) = webrtc_controller.deinit().await {
                                        log::error!("webrtc deinit failed: {e}");
                                    }
                                    // terminate the task on purpose.
                                    return;
                                }
                            }
                            EmittedEvents::Disconnected { peer }
                            | EmittedEvents::ConnectionFailed { peer } => {
                                // todo: could need to retry
                                active_call.connected_participants.insert(peer.clone(), PeerState::Disconnected);
                                if let Err(e) = host_media::remove_sink_track(peer.clone()).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                                webrtc_controller.hang_up(&peer).await;
                            }
                            EmittedEvents::CallInitiated { dest, sdp } => {
                                let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                                let signal = PeerSignal::Dial(*sdp);
                                if let Err(e) = send_signal_ecdh(ipfs, own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Sdp { dest, sdp } => {
                                let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                                let signal = PeerSignal::Sdp(*sdp);
                                if let Err(e) = send_signal_ecdh(ipfs, own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Ice { dest, candidate } => {
                               let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                               let signal = PeerSignal::Ice(*candidate);
                                if let Err(e) = send_signal_ecdh(ipfs, own_id, &dest, signal, topic).await {
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

impl Extension for BlinkImpl {
    fn id(&self) -> String {
        "warp-blink-wrtc".to_string()
    }
    fn name(&self) -> String {
        "Blink WebRTC".into()
    }

    fn module(&self) -> Module {
        Module::Media
    }
}

impl SingleHandle for BlinkImpl {
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        Err(Error::Unimplemented)
    }
}

/// blink implementation
///
///
#[async_trait]
impl Blink for BlinkImpl {
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
        conversation_id: Option<Uuid>,
        mut participants: Vec<DID>,
        webrtc_codec: AudioCodec,
    ) -> Result<Uuid, Error> {
        if webrtc_codec.channels() != 1 {
            return Err(Error::OtherWithContext(
                "webrtc_codec must have only 1 channel".into(),
            ));
        }
        if self.ipfs.read().await.is_none() {
            return Err(Error::OtherWithContext(
                "received signal before blink is initialized".into(),
            ));
        }
        if let Some(ac) = self.active_call.read().await.as_ref() {
            if ac.call_state != CallState::Closed {
                return Err(Error::OtherWithContext("previous call not finished".into()));
            }
        }

        // need to drop the lock before calling self.init() so that self doesn't have 2 mutable borrows.
        {
            let lock = self.own_id.read().await;
            let own_id = match lock.as_ref() {
                Some(r) => r,
                None => {
                    return Err(Error::OtherWithContext(
                        "received signal before blink is initialized".into(),
                    ));
                }
            };

            if !participants.contains(own_id) {
                participants.push(DID::from_str(&own_id.fingerprint())?);
            };
        }

        let call_info = CallInfo::new(conversation_id, participants.clone(), webrtc_codec);
        self.init_call(call_info.clone()).await?;

        let lock = self.own_id.read().await;
        let own_id = match lock.as_ref() {
            Some(r) => r,
            None => {
                return Err(Error::OtherWithContext(
                    "received signal before blink is initialized".into(),
                ));
            }
        };
        let lock = self.ipfs.read().await;
        let ipfs = match lock.as_ref() {
            Some(r) => r,
            None => {
                return Err(Error::OtherWithContext(
                    "received signal before blink is initialized".into(),
                ));
            }
        };
        for dest in participants {
            if dest == *own_id {
                continue;
            }
            let topic = ipfs_routes::call_initiation_route(&dest);
            let signal = InitiationSignal::Offer {
                call_info: call_info.clone(),
            };

            if let Err(e) = send_signal_ecdh(ipfs, own_id, &dest, signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
        }
        Ok(call_info.call_id())
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if self.ipfs.read().await.is_none() {
            return Err(Error::OtherWithContext(
                "received signal before blink is initialized".into(),
            ));
        }
        if let Some(ac) = self.active_call.read().await.as_ref() {
            if ac.call_state != CallState::Closed {
                return Err(Error::OtherWithContext("previous call not finished".into()));
            }
        }

        let call = match self.pending_calls.write().await.remove(&call_id) {
            Some(r) => r,
            None => {
                return Err(Error::OtherWithContext(
                    "could not answer call: not found".into(),
                ))
            }
        };

        self.init_call(call.clone()).await?;
        let call_id = call.call_id();
        let topic = ipfs_routes::call_signal_route(&call_id);
        let signal = CallSignal::Join { call_id };

        let lock = self.ipfs.read().await;
        let ipfs = match lock.as_ref() {
            Some(r) => r,
            None => {
                // should never happen
                return Err(Error::OtherWithContext(
                    "received signal before blink is initialized".into(),
                ));
            }
        };
        if let Err(e) = send_signal_aes(ipfs, &call.group_key(), signal, topic).await {
            log::error!("failed to send signal: {e}");
        }
        Ok(())
    }
    /// use the Leave signal as a courtesy, to let the group know not to expect you to join.
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let lock = self.ipfs.read().await;
        let ipfs = match lock.as_ref() {
            Some(r) => r,
            None => {
                return Err(Error::OtherWithContext(
                    "received signal before blink is initialized".into(),
                ));
            }
        };
        if let Some(call) = self.pending_calls.write().await.remove(&call_id) {
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };
            if let Err(e) = send_signal_aes(ipfs, &call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
            Ok(())
        } else {
            Err(Error::OtherWithContext(
                "could not reject call: not found".into(),
            ))
        }
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        let lock = self.ipfs.read().await;
        let ipfs = match lock.as_ref() {
            Some(r) => r,
            None => {
                return Err(Error::OtherWithContext(
                    "received signal before blink is initialized".into(),
                ));
            }
        };
        if let Some(ac) = self.active_call.write().await.as_mut() {
            match ac.call_state.clone() {
                CallState::Started => {
                    ac.call_state = CallState::Closing;
                }
                CallState::Closed => {
                    log::info!("call already closed");
                    return Ok(());
                }
                CallState::Uninitialized => {
                    log::info!("cancelling call");
                    ac.call_state = CallState::Closed;
                }
                CallState::Closing => {
                    log::warn!("leave_call when call_state is: {:?}", ac.call_state);
                    return Ok(());
                }
            };

            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };
            if let Err(e) = send_signal_aes(ipfs, &ac.call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to leave call");
            }

            let r = self.webrtc_controller.write().await.deinit().await;
            host_media::reset().await;
            let _ = r?;
            Ok(())
        } else {
            Err(Error::OtherWithContext(
                "tried to leave nonexistent call".into(),
            ))
        }
    }

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error> {
        let device_iter = match cpal::default_host().input_devices() {
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
        let device_iter = match cpal::default_host().output_devices() {
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
        Err(Error::Unimplemented)
    }
    async fn select_camera(&mut self, _device_name: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn select_default_camera(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
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
        Err(Error::Unimplemented)
    }
    async fn disable_camera(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn record_call(&mut self, _output_dir: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn stop_recording(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn get_audio_source_codec(&self) -> AudioCodec {
        self.audio_source_codec.read().await.clone()
    }
    async fn set_audio_source_codec(&mut self, codec: AudioCodec) -> Result<(), Error> {
        *self.audio_source_codec.write().await = codec;
        // todo: validate the codec
        Ok(())
    }
    async fn get_audio_sink_codec(&self) -> AudioCodec {
        self.audio_sink_codec.read().await.clone()
    }
    async fn set_audio_sink_codec(&mut self, codec: AudioCodec) -> Result<(), Error> {
        *self.audio_sink_codec.write().await = codec;
        // todo: validate the codec
        Ok(())
    }

    // ------ Utility Functions ------

    async fn pending_calls(&self) -> Vec<CallInfo> {
        Vec::from_iter(self.pending_calls.read().await.values().cloned())
    }
    async fn current_call(&self) -> Option<CallInfo> {
        self.active_call
            .read()
            .await
            .as_ref()
            .map(|x| x.call.clone())
    }

    // ------ Config ------

    async fn config_audio_processing(
        &mut self,
        config: AudioProcessingConfig,
    ) -> Result<(), Error> {
        host_media::config_audio_processing(config).await;
        Ok(())
    }
}
