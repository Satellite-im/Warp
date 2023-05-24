//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!
//! The init() function must be called prior to using the Blink implementation.
//! the deinit() function must be called to ensure all threads are cleaned up properly.
//!
//! init() returns a BlinkImpl struct, which as the name suggests, implements Blink.
//! All data used by the implementation is contained in two static variables: IPFS and BLINK_DATA.
//!

mod host_media;
mod signaling;
mod simple_webrtc;
mod store;

use async_trait::async_trait;
use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

use anyhow::{bail, Context};
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
    blink::{self, AudioCodec, Blink, BlinkEventKind, BlinkEventStream, CallInfo, MimeType},
    crypto::{Fingerprint, DID},
    error::Error,
    multipass::MultiPass,
};

use crate::{
    signaling::{ipfs_routes, CallSignal, InitiationSignal, PeerSignal},
    simple_webrtc::events::{EmittedEvents, WebRtcEventStream},
    store::{
        decode_gossipsub_msg_aes, decode_gossipsub_msg_ecdh, send_signal_aes, send_signal_ecdh,
        PeerIdExt,
    },
};

// basically a singleton
// doesn't bother the user with having to use a mutex
// implements Blink
#[derive(Clone)]
pub struct BlinkImpl {}

impl Drop for BlinkImpl {
    fn drop(&mut self) {
        tokio::spawn(async {
            deinit().await;
        });
    }
}

static IPFS: Lazy<Mutex<Option<Ipfs>>> = Lazy::new(|| Mutex::new(None));
static BLINK_DATA: Lazy<Mutex<BlinkData>> = Lazy::new(|| {
    let (ui_event_ch, _rx) = broadcast::channel(1024);
    let webrtc = simple_webrtc::Controller::new().expect("failed to create webrtc controller");

    let cpal_host = cpal::default_host();

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

    Mutex::new(BlinkData {
        webrtc_controller: webrtc,
        ui_event_ch,
        own_id: DID::default(),
        cpal_host: cpal::default_host(),
        active_call: None,
        pending_calls: HashMap::new(),
        // todo: validate the source and sink codecs and set them to None if invalid
        audio_source_codec: Some(source_codec),
        audio_sink_codec: Some(sink_codec),
        _account: None,
        id: Arc::new(DID::default()),
        offer_handler: None,
        webrtc_handler: None,
    })
});

struct BlinkData {
    webrtc_controller: simple_webrtc::Controller,
    own_id: DID,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    active_call: Option<ActiveCall>,
    pending_calls: HashMap<Uuid, CallInfo>,
    // todo: maybe get rid of this
    cpal_host: cpal::Host,
    audio_source_codec: Option<blink::AudioCodec>,
    audio_sink_codec: Option<blink::AudioCodec>,

    _account: Option<Box<dyn MultiPass>>,
    id: Arc<DID>,
    // subscribes to IPFS topic to receive incoming calls
    offer_handler: Option<JoinHandle<()>>,
    // handles 3 streams: one for webrtc events and two IPFS topics
    // pertains to the active_call, which is stored in STATIC_DATA
    webrtc_handler: Option<JoinHandle<()>>,
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

pub async fn deinit() {
    let mut data = BLINK_DATA.lock().await;
    let mut ipfs = IPFS.lock().await;
    if let Some(handle) = data.webrtc_handler.take() {
        handle.abort();
    }
    if let Some(handle) = data.offer_handler.take() {
        handle.abort();
    }
    let _ = data.webrtc_controller.deinit().await;
    ipfs.take();
    log::debug!("deinit finished");
}

pub async fn init(account: Box<dyn MultiPass>) -> anyhow::Result<Box<BlinkImpl>> {
    log::trace!("initializing WebRTC");
    let mut data = BLINK_DATA.lock().await;
    let mut global_ipfs = IPFS.lock().await;
    if global_ipfs.is_some() {
        bail!("blink is already initialized");
    }
    let identity = loop {
        if let Ok(identity) = account.get_own_identity().await {
            break identity;
        }
        tokio::time::sleep(Duration::from_millis(100)).await
    };
    let did = identity.did_key();
    data.own_id = did.clone();

    let ipfs_handle = match account.handle() {
        Ok(handle) if handle.is::<Ipfs>() => handle.downcast_ref::<Ipfs>().cloned(),
        _ => anyhow::bail!("Unable to obtain IPFS Handle"),
    };

    let ipfs = match ipfs_handle {
        Some(r) => r,
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

    global_ipfs.replace(ipfs);
    data._account = Some(account);
    data.id = Arc::new(did);
    data.offer_handler = Some(offer_handler);

    log::trace!("finished initializing WebRTC");
    Ok(Box::new(BlinkImpl {}))
}

async fn init_call(
    data: &mut BlinkData,
    ipfs: &Ipfs,
    call: CallInfo,
    audio_source_codec: AudioCodec,
) -> anyhow::Result<()> {
    data.active_call.replace(call.clone().into());
    // ensure there is an audio source track
    let rtc_rtp_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
        mime_type: audio_source_codec.mime_type(),
        clock_rate: audio_source_codec.sample_rate(),
        channels: audio_source_codec.channels(),
        ..Default::default()
    };
    let track = data
        .webrtc_controller
        .add_media_source(host_media::AUDIO_SOURCE_ID.into(), rtc_rtp_codec)
        .await?;
    host_media::create_audio_source_track(track, call.codec(), audio_source_codec).await?;

    // next, create event streams and pass them to a task
    let call_broadcast_stream = ipfs
        .pubsub_subscribe(ipfs_routes::call_signal_route(&call.id()))
        .await
        .context("failed to subscribe to call_broadcast_route")?;

    let call_signaling_stream = ipfs
        .pubsub_subscribe(ipfs_routes::peer_signal_route(&data.id, &call.id()))
        .await
        .context("failed to subscribe to call_signaling_route")?;

    let webrtc_event_stream = WebRtcEventStream(Box::pin(
        data.webrtc_controller
            .get_event_stream()
            .context("failed to get webrtc event stream")?,
    ));

    if let Some(handle) = data.webrtc_handler.take() {
        // just to be safe
        handle.abort();
    }

    let ui_event_ch = data.ui_event_ch.clone();
    let own_id = data.id.clone();
    let ipfs2 = ipfs.clone();
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

    data.webrtc_handler.replace(webrtc_handle);
    Ok(())
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

                let mut data = BLINK_DATA.lock().await;
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
                let mut data = BLINK_DATA.lock().await;
                let active_call = match data.active_call.as_mut() {
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
                            x @ _ => if x != CallState::Started {
                                     log::error!("someone tried to join call with state: {:?}", active_call.call_state);
                                    continue;
                            }
                        }
                        active_call.connected_participants.insert(sender.clone(), PeerState::Initializing);
                        // todo: properly hang up on error.
                        // emits CallInitiated Event, which returns the local sdp. will be sent to the peer with the dial signal
                        if let Err(e) = data.webrtc_controller.dial(&sender).await {
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
                        if active_call.call.id() != call_id {
                            log::error!("participant tried to leave call which wasn't active");
                            continue;
                        }
                        if !active_call.call.participants().contains(&sender) {
                            log::error!("participant tried to leave call who wasn't part of the call");
                            continue;
                        }
                        data.webrtc_controller.hang_up(&sender).await;
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
                let mut data = BLINK_DATA.lock().await;
                let active_call = match data.active_call.as_mut() {
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

                match signal {
                    PeerSignal::Ice(ice) => {
                        if active_call.call_state != CallState::Started {
                            log::error!("ice received for uninitialized call");
                            continue;
                        }
                        if let Err(e) = data.webrtc_controller.recv_ice(&sender, ice).await {
                            log::error!("failed to recv_ice {}", e);
                        }
                    }
                    PeerSignal::Sdp(sdp) => {
                        if active_call.call_state != CallState::Started {
                            log::error!("sdp received for uninitialized call");
                            continue;
                        }
                        log::debug!("received signal: SDP");
                        if let Err(e) = data.webrtc_controller.recv_sdp(&sender, sdp).await {
                            log::error!("failed to recv_sdp: {}", e);
                        }
                    }
                    PeerSignal::Dial(sdp) => {
                        if active_call.call_state == CallState::Uninitialized {
                            active_call.call_state = CallState::Started;
                        }
                        log::debug!("received signal: Dial");
                        // emits the SDP Event, which is sent to the peer via the SDP signal
                        if let Err(e) = data.webrtc_controller.accept_call(&sender, sdp).await {
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
                        let mut data = BLINK_DATA.lock().await;
                        let active_call = match data.active_call.as_mut() {
                            Some(ac) => ac,
                            None => {
                                log::error!("event emitted but no active call");
                                continue;
                            }
                        };
                        let call_id = active_call.call.id();
                        match event {
                            EmittedEvents::TrackAdded { peer, track } => {
                                let webrtc_codec = active_call.call.codec();
                                if peer == data.own_id {
                                    log::warn!("got TrackAdded event for own id");
                                    continue;
                                }
                                let audio_sink_codec = match data.audio_sink_codec.clone() {
                                    Some(r) => r,
                                    None => {
                                        log::error!("can not add track - no audio sink codec");
                                        continue;
                                    }
                                };
                                if let Err(e) =   host_media::create_audio_sink_track(peer.clone(), track, webrtc_codec, audio_sink_codec).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                            }
                            EmittedEvents::Connected { peer } => {
                                active_call.connected_participants.insert(peer, PeerState::Connected);
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
                                data.webrtc_controller.hang_up(&peer).await;
                                // this log should appear after the logs emitted by hang_up
                                if all_closed {
                                    log::info!("all participants have successfully been disconnected");
                                    if let Err(e) = data.webrtc_controller.deinit().await {
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
                                data.webrtc_controller.hang_up(&peer).await;
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

/// blink implementation
///
///
#[async_trait]
impl Blink for BlinkImpl {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    async fn get_event_stream(&mut self) -> Result<BlinkEventStream, Error> {
        let data = BLINK_DATA.lock().await;
        let mut rx = data.ui_event_ch.subscribe();
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
        webrtc_codec: AudioCodec,
    ) -> Result<(), Error> {
        let mut data = BLINK_DATA.lock().await;
        let ipfs = IPFS.lock().await;
        let ipfs = match ipfs.as_ref() {
            Some(r) => r,
            None => return Err(anyhow::anyhow!("ipfs not initialized").into()),
        };
        if let Some(ac) = data.active_call.as_ref() {
            if ac.call_state != CallState::Closed {
                return Err(Error::OtherWithContext("previous call not finished".into()));
            }
        }
        let audio_source_codec = data
            .audio_source_codec
            .clone()
            .ok_or(Error::OtherWithContext(
                "Audio source codec not specified".into(),
            ))?;
        if data.audio_sink_codec.is_none() {
            return Err(Error::OtherWithContext(
                "Audio sink codec not specified".into(),
            ));
        }
        if !participants.contains(&data.id) {
            participants.push(DID::from_str(&data.id.fingerprint())?);
        };
        let call_info = CallInfo::new(participants.clone(), webrtc_codec);
        init_call(&mut data, &ipfs, call_info.clone(), audio_source_codec).await?;
        for dest in participants {
            if dest == *data.id {
                continue;
            }
            let topic = ipfs_routes::call_initiation_route(&dest);
            let signal = InitiationSignal::Offer {
                call_info: call_info.clone(),
            };

            if let Err(e) = send_signal_ecdh(&ipfs, &data.id, &dest, signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
        }
        Ok(())
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let mut data = BLINK_DATA.lock().await;
        let ipfs = IPFS.lock().await;
        let ipfs = match ipfs.as_ref() {
            Some(r) => r,
            None => return Err(anyhow::anyhow!("ipfs not initialized").into()),
        };
        if let Some(ac) = data.active_call.as_ref() {
            if ac.call_state != CallState::Closed {
                return Err(Error::OtherWithContext("previous call not finished".into()));
            }
        }
        let audio_source_codec = data
            .audio_source_codec
            .clone()
            .ok_or(Error::OtherWithContext(
                "Audio source codec not specified".into(),
            ))?;
        if data.audio_sink_codec.is_none() {
            return Err(Error::OtherWithContext(
                "Audio sink codec not specified".into(),
            ));
        }
        if let Some(call) = data.pending_calls.remove(&call_id) {
            init_call(&mut data, &ipfs, call.clone(), audio_source_codec).await?;
            let call_id = call.id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Join { call_id };
            if let Err(e) = send_signal_aes(&ipfs, &call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
            Ok(())
        } else {
            Err(Error::OtherWithContext(
                "could not answer call: not found".into(),
            ))
        }
    }
    /// use the Leave signal as a courtesy, to let the group know not to expect you to join.
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let mut data = BLINK_DATA.lock().await;
        let ipfs = IPFS.lock().await;
        let ipfs = match ipfs.as_ref() {
            Some(r) => r,
            None => return Err(anyhow::anyhow!("ipfs not initialized").into()),
        };
        if let Some(call) = data.pending_calls.remove(&call_id) {
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };
            if let Err(e) = send_signal_aes(&ipfs, &call.group_key(), signal, topic).await {
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
        let mut data = BLINK_DATA.lock().await;
        let ipfs = IPFS.lock().await;
        let ipfs = match ipfs.as_ref() {
            Some(r) => r,
            None => return Err(anyhow::anyhow!("ipfs not initialized").into()),
        };
        if let Some(ac) = data.active_call.as_mut() {
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

            let call_id = ac.call.id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };
            if let Err(e) = send_signal_aes(&ipfs, &ac.call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to leave call");
            }

            let r = data.webrtc_controller.deinit().await;
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
        let data = BLINK_DATA.lock().await;
        let device_iter = match data.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn get_current_microphone(&self) -> Option<String> {
        let _data = BLINK_DATA.lock().await;
        host_media::get_input_device_name().await
    }
    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
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
        let _data = BLINK_DATA.lock().await;
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
        let data = BLINK_DATA.lock().await;
        let device_iter = match data.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn get_current_speaker(&self) -> Option<String> {
        let _data = BLINK_DATA.lock().await;
        host_media::get_output_device_name().await
    }
    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
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
        let _data = BLINK_DATA.lock().await;
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
        let _data = BLINK_DATA.lock().await;
        todo!()
    }
    async fn select_camera(&mut self, _device_name: &str) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        todo!()
    }

    async fn select_default_camera(&mut self) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        todo!()
    }

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        host_media::mute_self()
            .await
            .map_err(|e| warp::error::Error::OtherWithContext(e.to_string()))
    }
    async fn unmute_self(&mut self) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        host_media::unmute_self()
            .await
            .map_err(|e| warp::error::Error::OtherWithContext(e.to_string()))
    }
    async fn enable_camera(&mut self) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        todo!()
    }
    async fn disable_camera(&mut self) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        todo!()
    }
    async fn record_call(&mut self, _output_file: &str) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        todo!()
    }
    async fn stop_recording(&mut self) -> Result<(), Error> {
        let _data = BLINK_DATA.lock().await;
        todo!()
    }

    async fn get_audio_source_codec(&self) -> Option<AudioCodec> {
        let data = BLINK_DATA.lock().await;
        data.audio_source_codec.clone()
    }
    async fn set_audio_source_codec(&mut self, codec: AudioCodec) -> Result<(), Error> {
        let mut data = BLINK_DATA.lock().await;
        data.audio_source_codec.replace(codec);
        // todo: validate the codec
        Ok(())
    }
    async fn get_audio_sink_codec(&self) -> Option<AudioCodec> {
        let data = BLINK_DATA.lock().await;
        data.audio_sink_codec.clone()
    }
    async fn set_audio_sink_codec(&mut self, codec: AudioCodec) -> Result<(), Error> {
        let mut data = BLINK_DATA.lock().await;
        data.audio_sink_codec.replace(codec);
        // todo: validate the codec
        Ok(())
    }

    // ------ Utility Functions ------

    async fn pending_calls(&self) -> Vec<CallInfo> {
        let data = BLINK_DATA.lock().await;
        Vec::from_iter(data.pending_calls.values().cloned())
    }
    async fn current_call(&self) -> Option<CallInfo> {
        let data = BLINK_DATA.lock().await;
        data.active_call.as_ref().map(|x| x.call.clone())
    }
}
