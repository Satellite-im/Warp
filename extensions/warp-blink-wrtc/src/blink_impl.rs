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
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Context};
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;
use ipfs::{libp2p::gossipsub::GossipsubMessage, Ipfs, IpfsTypes, SubscriptionStream};
use once_cell::sync::Lazy;
use rand::rngs::OsRng;
use serde::de::DeserializeOwned;
use tokio::{
    sync::{
        broadcast::{self, Sender},
        Mutex, Notify,
    },
    task::JoinHandle,
};
use uuid::Uuid;
use warp::{
    blink::{BlinkEventKind, CallInfo},
    crypto::{aes_gcm::Aes256Gcm, digest::KeyInit, DID},
    multipass::MultiPass,
    sata::Sata,
    sync::RwLock,
};

use crate::{
    host_media, ipfs_routes,
    signaling::{CallSignal, PeerSignal},
    simple_webrtc::{
        self,
        events::{EmittedEvents, WebRtcEventStream},
        Controller,
    },
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

pub struct WebRtc<T: IpfsTypes> {
    account: Box<dyn MultiPass>,
    ipfs: Arc<RwLock<Ipfs<T>>>,
    id: DID,
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

impl<T: IpfsTypes> WebRtc<T> {
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
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
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
        let offer_handler = tokio::spawn(async {
            handle_call_offers(own_id, call_offer_stream, ui_event_ch2).await;
        });

        let webrtc = Self {
            account,
            ipfs: Arc::new(RwLock::new(ipfs.clone())),
            id: did.clone(),
            ui_event_ch,
            offer_handler,
            webrtc_handler: None,
        };

        Ok(webrtc)
    }

    // todo: make sure this only gets called once
    async fn init_call(&mut self, call: CallInfo, stop: Arc<Notify>) -> anyhow::Result<()> {
        let mut _data = STATIC_DATA.lock().await;

        // this will cause the ipfs streams to be dropped and unsubscribe from the topics
        if let Some(handle) = self.webrtc_handler.take() {
            handle.abort();
        }
        // there is no longer an active call
        _data.active_call.take();

        // ensure there is no ongoing webrtc call
        _data
            .webrtc
            .deinit()
            .await
            .context("webrtc deinit failed")?;

        // next, create event streams and pass them to a task
        let ipfs = self.ipfs.read();
        let call_broadcast_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_broadcast_route(&call.id()))
            .await
            .context("failed to subscribe to call_broadcast_route")?;

        let call_signaling_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_signal_route(&self.id, &call.id()))
            .await
            .context("failed to subscribe to call_signaling_route")?;

        let webrtc_event_stream = WebRtcEventStream(Box::pin(
            _data
                .webrtc
                .get_event_stream()
                .context("failed to get webrtc event stream")?,
        ));

        let ui_event_ch = self.ui_event_ch.clone();
        let own_id = self.id.clone();
        let webrtc_handle = tokio::task::spawn(async move {
            handle_webrtc(
                own_id,
                ui_event_ch,
                call_broadcast_stream,
                call_signaling_stream,
                webrtc_event_stream,
            )
            .await;
        });

        self.webrtc_handler.replace(webrtc_handle);
        _data.active_call.replace(call.into());

        Ok(())
    }

    async fn cleanup_call(&mut self) {}
}

fn decode_gossipsub_msg<T: DeserializeOwned>(
    own_id: &DID,
    opt: Option<Arc<GossipsubMessage>>,
) -> anyhow::Result<T> {
    let msg = match opt {
        Some(s) => s,
        None => bail!("message was None"),
    };
    let encoded_data = match serde_cbor::from_slice::<Sata>(&msg.data) {
        Ok(d) => d,
        Err(e) => {
            bail!("failed to decode payload: {e}");
        }
    };

    let data: T = match encoded_data.decrypt(own_id) {
        Ok(d) => d,
        Err(e) => {
            bail!("failed to decrypt payload: {e}");
        }
    };

    Ok(data)
}

async fn handle_call_offers(
    own_id: DID,
    mut stream: SubscriptionStream,
    ch: Sender<BlinkEventKind>,
) {
    while let Some(msg) = stream.next().await {
        // todo: decrypt
        let call_info: CallInfo = match decode_gossipsub_msg(&own_id, Some(msg.clone())) {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to decode msg from call signaling stream: {e}");
                continue;
            }
        };

        let evt = BlinkEventKind::IncomingCall {
            call_id: call_info.id(),
            sender: call_info.sender(),
            participants: call_info.participants(),
        };

        let mut data: tokio::sync::MutexGuard<StaticData> = STATIC_DATA.lock().await;
        data.pending_calls.insert(call_info.id(), call_info);
        ch.send(evt);
    }
}

async fn handle_webrtc(
    own_id: DID,
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
                let signal: CallSignal = match decode_gossipsub_msg(&own_id, opt) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };
                match signal {
                    CallSignal::Join { call_id } => {
                        // todo: initiate connection
                    }
                    CallSignal::Leave { call_id } => {
                        // todo: don't retry
                    },
                    CallSignal::Reject { call_id } => {
                       // todo: anything?
                    }
                }
            },
            opt = peer_signaling_stream.next() => {
                // todo: decrypt
                let signal: PeerSignal = match decode_gossipsub_msg(&own_id, opt) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("failed to decode msg from call signaling stream: {e}");
                        continue;
                    },
                };

                match signal {
                    PeerSignal::Ice(icd) => {}
                    PeerSignal::Sdp(sdp) => {}
                    PeerSignal::CallInitiated(sdp) => {
                    }
                    PeerSignal::CallTerminated(call_id) => {}
                    PeerSignal::CallRejected(call_id) => {}
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

                                ch.send(BlinkEventKind::ParticipantJoined { call_id, peer_id: peer });
                            }
                            EmittedEvents::Disconnected { peer } => {
                                let mut _data = STATIC_DATA.lock().await;
                                // todo: retry unless the hangup signal was sent
                                if let Err(e) = host_media::remove_sink_track(peer.clone()).await {
                                    log::error!("failed to send media_track command: {e}");
                                }

                                _data.webrtc.hang_up(&peer).await;
                            }
                            // todo: store the (dest, sdp) pair and let the UI decide what to do about it.
                            EmittedEvents::CallInitiated { dest, sdp } => {
                                let mut _data = STATIC_DATA.lock().await;
                                if !_data.active_call.as_ref().map(|ac| ac.call.participants().contains(&dest)).unwrap_or(false) {
                                    todo!("send signal to reject call");
                                }

                                if let Err(e) = _data.webrtc.accept_call(&dest, *sdp).await {
                                    log::error!("failed to accept_call: {}", e);
                                    _data.webrtc.hang_up(&dest).await;
                                    // todo: is a disconnect signal needed here?
                                }
                            }
                            EmittedEvents::Sdp { dest, sdp } => {
                                let _data = STATIC_DATA.lock().await;
                                if let Err(e) = _data.webrtc.recv_sdp(&dest, *sdp).await {
                                    log::error!("failed to recv_sdp: {}", e);
                                }
                            }
                            EmittedEvents::Ice { dest, candidate } => {
                                let _data = STATIC_DATA.lock().await;
                                if let Err(e) = _data.webrtc.recv_ice(&dest, *candidate).await {
                                    log::error!("failed to recv_ice {}", e);
                                }
                            }
                        }
                        todo!("handle event");
                    }
                    None => todo!()
                }
            }
        }
    }
}
