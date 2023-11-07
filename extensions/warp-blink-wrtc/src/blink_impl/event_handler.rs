use futures::channel::oneshot;
use futures::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};
use tokio::{
    sync::{
        broadcast,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Notify,
    },
    time::Instant,
};
use uuid::Uuid;
use warp::{
    blink::{BlinkEventKind, CallConfig, CallInfo, ParticipantState},
    crypto::{cipher::Cipher, DID},
    sync::RwLock,
};
use webrtc::{
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::{
    blink_impl::data::{CallState, PeerState},
    host_media::{
        self,
        audio::{AudioCodec, AudioHardwareConfig},
    },
    signaling::{ipfs_routes, GossipSubSignal, PeerSignal},
    simple_webrtc::{self, events::WebRtcEventStream, MediaSourceId},
};

use super::{data::NotifyWrapper, gossipsub_sender::GossipSubSender};

enum EventHandlerCmd {
    SetActiveCall {
        call_info: CallInfo,
    },
    AddMediaSource {
        source_id: MediaSourceId,
        codec: RTCRtpCodecCapability,
        rsp: oneshot::Sender<anyhow::Result<Arc<TrackLocalStaticRTP>>>,
    },
    RemoveMediaSource {
        source_id: MediaSourceId,
    },
    LeaveCall,
}

#[derive(Clone)]
pub struct EventHandler {
    ch: UnboundedSender<EventHandlerCmd>,
    notify: Arc<NotifyWrapper>,
}

impl EventHandler {
    pub fn new(
        webrtc_controller: simple_webrtc::Controller,
        webrtc_event_stream: WebRtcEventStream,
        gossipsub_sender: GossipSubSender,
        signal_rx: UnboundedReceiver<GossipSubSignal>,
        event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Self {
        let (tx, cmd_rx) = mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let notify2 = notify.clone();
        tokio::spawn(async move {
            run(
                webrtc_controller,
                webrtc_event_stream,
                gossipsub_sender,
                cmd_rx,
                signal_rx,
                event_ch,
                notify2,
            )
            .await;
        });
        Self {
            ch: tx,
            notify: Arc::new(NotifyWrapper { notify }),
        }
    }

    pub fn set_active_call(&self, call_info: CallInfo) -> anyhow::Result<()> {
        self.ch.send(EventHandlerCmd::SetActiveCall { call_info })?;
        Ok(())
    }

    pub async fn add_media_source(
        &self,
        source_id: MediaSourceId,
        codec: RTCRtpCodecCapability,
    ) -> anyhow::Result<Arc<TrackLocalStaticRTP>> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(EventHandlerCmd::AddMediaSource {
            source_id,
            codec,
            rsp: tx,
        })?;
        rx.await?
    }

    pub fn remove_media_source(&self, source_id: MediaSourceId) -> anyhow::Result<()> {
        self.ch
            .send(EventHandlerCmd::RemoveMediaSource { source_id })?;
        Ok(())
    }

    pub fn leave_call(&self) -> anyhow::Result<()> {
        self.ch.send(EventHandlerCmd::LeaveCall)?;
        Ok(())
    }
}

async fn run(
    mut webrtc_controller: simple_webrtc::Controller,
    mut webrtc_event_stream: WebRtcEventStream,
    gossipsub_sender: GossipSubSender,
    mut cmd_rx: UnboundedReceiver<EventHandlerCmd>,
    mut signal_rx: UnboundedReceiver<GossipSubSignal>,
    event_ch: broadcast::Sender<BlinkEventKind>,
    notify: Arc<Notify>,
) {
    let mut call_configs: HashMap<Uuid, CallConfig> = HashMap::new();
    let mut call_infos: HashMap<Uuid, CallInfo> = HashMap::new();
    let mut active_call: Option<Uuid> = None;
    let mut active_call_state = CallState::Uninitialized;

    loop {
        tokio::select! {
            _ = notify.notified() => {
                log::debug!("quitting blink event handler");
                break;
            },
            opt = cmd_rx.recv() => {
                let cmd = match opt {
                    Some(r) => r,
                    None => {
                        log::debug!("blink handler cmd_rx channel is closed. quitting");
                        break;
                    }
                };
                match cmd {
                    EventHandlerCmd::SetActiveCall { call_info } => {
                        if active_call.replace(call_info.call_id()).is_some() {
                            webrtc_controller.deinit();
                            host_media::reset().await;
                        }
                        call_infos.insert(call_info.call_id(), call_info);
                    },
                    EventHandlerCmd::AddMediaSource { source_id, codec, rsp } => {
                        let r = webrtc_controller.add_media_source(source_id, codec).await;
                        let _ = rsp.send(r);
                    },
                    EventHandlerCmd::RemoveMediaSource { source_id } => {
                        let _ = webrtc_controller.remove_media_source(source_id).await;
                    },
                    EventHandlerCmd::LeaveCall => {
                        if active_call.take().is_some() {
                            webrtc_controller.deinit();
                            host_media::reset().await;
                        }
                    },
                }
            },
            opt = signal_rx.recv() => {
                let cmd = match opt {
                    Some(r) => r,
                    None => {
                        log::debug!("blink handler signal_rx channel is closed. quitting");
                        break;
                    }
                };
                match cmd {
                    GossipSubSignal::Peer { sender, call_id, signal } => match signal {
                        _ if !matches!(active_call.as_ref(), Some(&call_id)) => {
                            log::debug!("received webrtc signal for non-active call");
                            continue;
                        }
                        _ if active_call_state != CallState::Started => {
                            log::debug!("received signal for uninitialized call: {:?}", std::mem::discriminant(&signal));
                            continue;
                        }
                        crate::signaling::PeerSignal::Ice(ice) => {
                            if let Err(e) = webrtc_controller.recv_ice(&sender, ice).await {
                                log::error!("failed to recv_ice {}", e);
                            }
                        },
                        crate::signaling::PeerSignal::Sdp(sdp) => {
                            log::debug!("received signal: SDP");
                            if let Err(e) = webrtc_controller.recv_sdp(&sender, sdp).await {
                                log::error!("failed to recv_sdp: {}", e);
                            }
                        },
                        crate::signaling::PeerSignal::Dial(sdp) => {
                            log::debug!("received signal: Dial");
                            // emits the SDP Event, which is sent to the peer via the SDP signal
                            if let Err(e) = webrtc_controller.accept_call(&sender, sdp).await {
                                log::error!("failed to accept_call: {}", e);
                            }
                        },
                    },
                    GossipSubSignal::Call { sender, call_id, signal } => match signal {
                        crate::signaling::CallSignal::Join => todo!(),
                        crate::signaling::CallSignal::Leave => todo!(),
                        crate::signaling::CallSignal::Muted => todo!(),
                        crate::signaling::CallSignal::Unmuted => todo!(),
                        crate::signaling::CallSignal::Deafened => todo!(),
                        crate::signaling::CallSignal::Undeafened => todo!(),
                    },
                    GossipSubSignal::Initiation { sender, signal } => match signal {
                        crate::signaling::InitiationSignal::Offer { call_info } => todo!(),
                    },
                }
            }
            opt = webrtc_event_stream.next() => {
                let event = match opt {
                    Some(r) => r,
                    None => {
                        log::debug!("webrtc_event_stream closed!");
                        // todo: get new webrtc controller or something
                        continue;
                    }
                };

                match event {
                    simple_webrtc::events::EmittedEvents::Ice { dest, candidate } => {
                        let topic = ipfs_routes::peer_signal_route(&dest, &active_call.unwrap_or_default());
                        let signal = PeerSignal::Ice(*candidate);
                        if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                            log::error!("failed to send signal: {e}");
                        }
                    },
                    simple_webrtc::events::EmittedEvents::Connected { peer } => {
                        let ac = active_call.unwrap_or_default();
                        if call_infos.get(&ac).map(|x| x.contains_participant(&peer)).unwrap_or_default() {
                            if let Some(config) = call_configs.get_mut(&ac) {
                                config.participants_joined.insert(peer, ParticipantState::default());
                            }
                        } else {
                            log::warn!("webrtc controller connected to a peer who wasn't in the list for the active call");
                            webrtc_controller.hang_up(&peer).await;
                        }
                    },
                    simple_webrtc::events::EmittedEvents::Disconnected { peer }
                    | simple_webrtc::events::EmittedEvents::ConnectionFailed { peer } => {
                        if let Some(config) = call_configs.get_mut(&active_call.unwrap_or_default()) {
                            config.participants_joined.remove(&peer);
                        }
                        if let Err(e) = host_media::remove_sink_track(peer.clone()).await {
                            log::error!("failed to send media_track command: {e}");
                        }
                        webrtc_controller.hang_up(&peer).await;
                    },
                    simple_webrtc::events::EmittedEvents::ConnectionClosed { peer } => todo!(),
                    simple_webrtc::events::EmittedEvents::Sdp { dest, sdp } => {
                        let topic = ipfs_routes::peer_signal_route(&dest, &active_call.unwrap_or_default());
                        let signal = PeerSignal::Sdp(*sdp);
                        if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                            log::error!("failed to send signal: {e}");
                        }
                    },
                    simple_webrtc::events::EmittedEvents::CallInitiated { dest, sdp } => {
                        let topic = ipfs_routes::peer_signal_route(&dest, &active_call.unwrap_or_default());
                        let signal = PeerSignal::Dial(*sdp);
                        if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                            log::error!("failed to send signal: {e}");
                        }
                    },
                    simple_webrtc::events::EmittedEvents::TrackAdded { peer, track } => {
                        if let Err(e) =   host_media::create_audio_sink_track(peer.clone(), event_ch.clone(), track, AudioCodec::default()).await {
                            log::error!("failed to send media_track command: {e}");
                        }
                    },
                }
            }
        }
    }
}
