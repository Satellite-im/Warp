//!
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
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Context};
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;
use once_cell::sync::Lazy;
use rand::rngs::OsRng;
use rust_ipfs::{
    libp2p::{
        self,
        gossipsub::{Gossipsub, GossipsubMessage},
    },
    Ipfs, SubscriptionStream,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::{
    sync::{
        broadcast::{self, Sender},
        Mutex, Notify, RwLock,
    },
    task::JoinHandle,
};
use uuid::Uuid;
use warp::{
    blink::{Blink, BlinkEventKind, BlinkEventStream, CallInfo},
    crypto::{aes_gcm::Aes256Gcm, digest::KeyInit, DIDKey, Ed25519KeyPair, KeyMaterial, DID},
    error::Error,
    multipass::MultiPass,
    sata::Sata,
};

use crate::{
    host_media,
    signaling::{CallSignal, InitiationSignal, PeerSignal},
    simple_webrtc::{
        self,
        events::{EmittedEvents, WebRtcEventStream},
        Controller,
    },
    store::{ecdh_encrypt, PeerIdExt},
};

#[derive(Clone)]
struct ActiveCall {
    call: CallInfo,
    connected_participants: HashSet<DID>,
    state: CallState,
}

// used when a call is accepted
impl From<CallInfo> for ActiveCall {
    fn from(value: CallInfo) -> Self {
        Self {
            call: value,
            state: CallState::InProgress,
            connected_participants: HashSet::new(),
        }
    }
}

#[derive(Clone)]
enum CallState {
    Pending,
    InProgress,
    Ended,
}

pub struct WebRtc {
    account: Box<dyn MultiPass>,
    ipfs: Arc<RwLock<Ipfs>>,
    id: DID,
    private_key: DID,
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
            host_media::change_audio_input(d).await;
        }
        if let Some(d) = cpal_host.default_output_device() {
            host_media::change_audio_output(d).await?;
        }

        let call_offer_stream = match ipfs
            .pubsub_subscribe(ipfs_routes::offer_call_route(&did))
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
        let own_id = did.clone();
        // todo: get private key from DID
        let private_key = todo!();
        let offer_handler = tokio::spawn(async {
            handle_call_initiation(own_id, private_key, call_offer_stream, ui_event_ch2).await;
        });

        let webrtc = Self {
            account,
            private_key,
            ipfs: Arc::new(RwLock::new(ipfs.clone())),
            id: did.clone(),
            ui_event_ch,
            offer_handler,
            webrtc_handler: None,
        };

        Ok(webrtc)
    }

    // todo: make sure this only gets called once
    async fn init_call(&mut self, data: &mut StaticData, call: CallInfo) -> anyhow::Result<()> {
        // this will cause the ipfs streams to be dropped and unsubscribe from the topics
        if let Some(handle) = self.webrtc_handler.take() {
            handle.abort();
        }
        // there is no longer an active call
        data.active_call.take();

        // ensure there is no ongoing webrtc call
        data.webrtc.deinit().await.context("webrtc deinit failed")?;

        // next, create event streams and pass them to a task
        let ipfs = self.ipfs.read().await;
        let call_broadcast_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_broadcast_route(&call.id()))
            .await
            .context("failed to subscribe to call_broadcast_route")?;

        let call_signaling_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_signal_route(&self.id, &call.id()))
            .await
            .context("failed to subscribe to call_signaling_route")?;

        let webrtc_event_stream = WebRtcEventStream(Box::pin(
            data.webrtc
                .get_event_stream()
                .context("failed to get webrtc event stream")?,
        ));

        let ui_event_ch = self.ui_event_ch.clone();
        let own_id = self.id.clone();
        let private_key = self.private_key.clone();
        let ipfs2 = self.ipfs.clone();
        let webrtc_handle = tokio::task::spawn(async move {
            handle_webrtc(
                own_id,
                private_key,
                ipfs2,
                ui_event_ch,
                call_broadcast_stream,
                call_signaling_stream,
                webrtc_event_stream,
            )
            .await;
        });

        self.webrtc_handler.replace(webrtc_handle);
        data.active_call.replace(call.into());

        Ok(())
    }

    async fn cleanup_call(&mut self) {}
}

fn decode_gossipsub_msg<T: DeserializeOwned>(
    private_key: &DID,
    msg: &libp2p::gossipsub::Message,
) -> anyhow::Result<T> {
    let bytes = crate::store::ecdh_decrypt(private_key, None, msg.data.clone())?;
    let data: T = serde_cbor::from_slice(&bytes)?;
    Ok(data)
}

async fn handle_call_initiation(
    own_id: DID,
    private_key: DID,
    mut stream: SubscriptionStream,
    ch: Sender<BlinkEventKind>,
) {
    while let Some(msg) = stream.next().await {
        let signal: InitiationSignal = match decode_gossipsub_msg(&private_key, &msg) {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to decode msg from call initiation stream: {e}");
                continue;
            }
        };

        let sender = match msg.source.and_then(|s| s.to_did().ok()) {
            Some(id) => id,
            None => {
                log::error!("msg received without source");
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
                ch.send(evt);
            }
            InitiationSignal::Reject {
                call_id,
                participant,
            } => {
                let mut data = STATIC_DATA.lock().await;
                // for direct calls, if they hang up, don't bother waiting.
                let no_answer = data
                    .active_call
                    .as_ref()
                    .map(|ac| {
                        let participants = ac.call.participants();
                        participants.len() == 2 && participants.iter().any(|id| id == &participant)
                    })
                    .unwrap_or(false);
                if no_answer {
                    if let Some(ac) = data.active_call.take() {
                        let evt = BlinkEventKind::CallEnded {
                            call_id: ac.call.id(),
                        };
                        ch.send(evt);
                    }
                }
            }
        }
    }
}

async fn handle_webrtc(
    own_id: DID,
    private_key: DID,
    ipfs: Arc<RwLock<Ipfs>>,
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
                let signal: CallSignal = match decode_gossipsub_msg(&private_key, &msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };
                match signal {
                    CallSignal::Join { call_id } => {
                        let mut data = STATIC_DATA.lock().await;
                        if let Some(ac) = data.active_call.as_mut() {
                            ac.connected_participants.insert(sender.clone());
                        }
                        ch.send(BlinkEventKind::ParticipantJoined { call_id, peer_id: sender });
                    }
                    CallSignal::Leave { call_id } => {
                        let mut data = STATIC_DATA.lock().await;
                        if let Some(ac) = data.active_call.as_mut() {
                            ac.connected_participants.remove(&sender);
                        }
                        ch.send(BlinkEventKind::ParticipantLeft { call_id, peer_id: sender });
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
                let mut data = STATIC_DATA.lock().await;
                let signal: PeerSignal = match decode_gossipsub_msg(&private_key, &msg) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };

                match signal {
                    PeerSignal::Ice(ice) => {
                        if let Err(e) = data.webrtc.recv_ice(&sender, ice).await {
                            log::error!("failed to recv_ice {}", e);
                        }
                        todo!()
                    }
                    PeerSignal::Sdp(sdp) => {
                        if let Err(e) = data.webrtc.recv_sdp(&sender, sdp).await {
                            log::error!("failed to recv_sdp: {}", e);
                        }
                        todo!()
                    }
                    PeerSignal::CallInitiated(sdp) => {
                        // if sender is part of ongoing call, start the call
                        if data.active_call.as_ref().map(|ac| ac.call.participants().contains(&sender)).unwrap_or(false) {
                            if let Err(e) = data.webrtc.accept_call(&sender, sdp).await {
                                log::error!("failed to accept_call: {}", e);
                                data.webrtc.hang_up(&sender).await;
                                // todo: is a disconnect signal needed here? perhaps a retry
                                todo!()
                            }
                        }
                    }
                }
            },
            opt = webrtc_event_stream.next() => {
                match opt {
                    Some(event) => {
                        log::debug!("webrtc event: {event}");
                        match event {
                            EmittedEvents::TrackAdded { peer, track } => {
                                let mut _data = STATIC_DATA.lock().await;
                                let call_id = match _data.active_call.as_ref() {
                                    Some(ac) => ac.call.id(),
                                    None => {
                                        log::error!("webrtc track added without an ongoing call");
                                        continue;
                                    }
                                };
                                if let Err(e) =   host_media::create_audio_sink_track(peer.clone(), track).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                            }
                            EmittedEvents::Disconnected { peer } => {
                                let mut data = STATIC_DATA.lock().await;
                                if let Err(e) = host_media::remove_sink_track(peer.clone()).await {
                                    log::error!("failed to send media_track command: {e}");
                                }
                                if  data.active_call.as_ref().map(|ac| ac.connected_participants.contains(&peer)).unwrap_or(false) {
                                    // todo: retry connection
                                } else {
                                    data.webrtc.hang_up(&peer).await;
                                }
                                todo!()
                            }
                            EmittedEvents::CallInitiated { dest, sdp } => {
                                let data = STATIC_DATA.lock().await;
                                let call_id = match data.active_call.as_ref() {
                                    Some(ac) => ac.call.id(),
                                    None => {
                                        log::error!("sdp event emitted but no active call");
                                        continue;
                                    }
                                };
                                let ipfs = ipfs.read().await;
                                let topic = ipfs_routes::call_signal_route(&dest, &call_id);
                                let signal = PeerSignal::CallInitiated(*sdp);
                                if let Err(e) = send_signal(&ipfs, dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Sdp { dest, sdp } => {
                                // need to transmit this to dest via signal
                                let data = STATIC_DATA.lock().await;
                                let call_id = match data.active_call.as_ref() {
                                    Some(ac) => ac.call.id(),
                                    None => {
                                        log::error!("sdp event emitted but no active call");
                                        continue;
                                    }
                                };
                                let ipfs = ipfs.read().await;
                                let topic = ipfs_routes::call_signal_route(&dest, &call_id);
                                let signal = PeerSignal::Sdp(*sdp);
                                if let Err(e) = send_signal(&ipfs, dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Ice { dest, candidate } => {
                               // need to transmit this to dest via signal
                               let data = STATIC_DATA.lock().await;
                               let call_id = match data.active_call.as_ref() {
                                   Some(ac) => ac.call.id(),
                                   None => {
                                       log::error!("sdp event emitted but no active call");
                                       continue;
                                   }
                               };
                               let ipfs = ipfs.read().await;
                               let topic = ipfs_routes::call_signal_route(&dest, &call_id);
                               let signal = PeerSignal::Ice(*candidate);
                                if let Err(e) = send_signal(&ipfs, dest, signal, topic).await {
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

async fn send_signal<T: Serialize>(
    ipfs: &Ipfs,
    dest: DID,
    signal: T,
    topic: String,
) -> anyhow::Result<()> {
    let serialized = serde_cbor::to_vec(&signal)?;
    let encrypted = ecdh_encrypt(&dest, Some(dest.clone()), serialized)?;
    ipfs.pubsub_publish(topic, encrypted).await?;
    Ok(())
}

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
    async fn offer_call(&mut self, participants: Vec<DID>) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        let call_info = CallInfo::new(participants.clone());
        let ac = ActiveCall::from(call_info.clone());
        if let Some(old_call) = data.active_call.replace(ac.clone()) {
            // todo: end call
        }

        self.init_call(&mut data, call_info.clone()).await?;

        let ipfs = self.ipfs.read().await;
        for dest in participants {
            let topic = ipfs_routes::offer_call_route(&dest);
            let signal = InitiationSignal::Offer {
                call_info: ac.call.clone(),
            };
            if let Err(e) = send_signal(&ipfs, dest, signal, topic).await {
                log::error!("failed to send signal: {e}");
            }
        }

        todo!();
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        if let Some(_call) = data.active_call.take() {
            // todo: end call
        }

        if let Some(call) = data.pending_calls.remove(&call_id) {
            self.init_call(&mut data, call).await?;
        }

        Ok(())
    }
    /// notify a sender/group that you will not join a call
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        if let Some(mut _call) = data.pending_calls.remove(&call_id) {
            // todo: signal
        }
        todo!()
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        let mut data = STATIC_DATA.lock().await;
        if let Some(ac) = data.active_call.take() {
            // todo: leave call

            data.webrtc.deinit().await?;

            // todo: remove media streams
        }
        todo!()
    }

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error> {
        let mut data = STATIC_DATA.lock().await;
        let device_iter = match data.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
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
                    host_media::change_audio_input(device).await;
                    return Ok(());
                }
            }
        }

        Err(warp::error::Error::OtherWithContext(
            "input device not found".into(),
        ))
    }
    async fn get_available_speakers(&self) -> Result<Vec<String>, Error> {
        let mut data = STATIC_DATA.lock().await;
        let device_iter = match data.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
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

    async fn pending_calls(&self) -> Vec<CallInfo> {
        let data = STATIC_DATA.lock().await;
        Vec::from_iter(data.pending_calls.values().cloned())
    }
    async fn current_call(&self) -> Option<CallInfo> {
        let data = STATIC_DATA.lock().await;
        data.active_call.as_ref().map(|x| x.call.clone())
    }
}
