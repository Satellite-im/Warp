use super::data::*;
use crate::host_media;
use crate::host_media::audio::AudioCodec;
use crate::simple_webrtc;
use futures::StreamExt;
use rust_ipfs::{Ipfs, SubscriptionStream};
use std::sync::Arc;
use tokio::sync::{
    broadcast::{self, Sender},
    RwLock,
};

use warp::{blink::BlinkEventKind, crypto::DID, error::Error};

use crate::{
    host_media::audio::AudioHardwareConfig,
    signaling::{ipfs_routes, CallSignal, PeerSignal},
    simple_webrtc::events::{EmittedEvents, WebRtcEventStream},
    store::{decode_gossipsub_msg_aes, decode_gossipsub_msg_ecdh, send_signal_ecdh, PeerIdExt},
};

pub struct WebRtcHandlerParams {
    pub own_id: Arc<RwLock<Option<DID>>>,
    pub event_ch: broadcast::Sender<BlinkEventKind>,
    pub ipfs: Ipfs,
    pub active_call: Arc<RwLock<Option<ActiveCall>>>,
    pub webrtc_controller: Arc<RwLock<simple_webrtc::Controller>>,
    pub audio_sink_config: Arc<RwLock<AudioHardwareConfig>>,
    pub ch: Sender<BlinkEventKind>,
    pub call_signaling_stream: SubscriptionStream,
    pub peer_signaling_stream: SubscriptionStream,
}

pub async fn run(params: WebRtcHandlerParams, mut webrtc_event_stream: WebRtcEventStream) {
    let WebRtcHandlerParams {
        own_id,
        event_ch,
        ipfs,
        active_call,
        webrtc_controller,
        audio_sink_config: audio_sink_codec,
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
                            log::error!("failed to send ParticipantLeft event: {e}");
                        }
                    },
                    CallSignal::Muted => {
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantMuted { peer_id: sender }) {
                            log::error!("failed to send ParticipantMuted event: {e}");
                        }
                    },
                    CallSignal::Unmuted => {
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantUnmuted { peer_id: sender }) {
                            log::error!("failed to send ParticipantUnmuted event: {e}");
                        }
                    }
                    CallSignal::Deafened => {
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantDeafened { peer_id: sender }) {
                            log::error!("failed to send ParticipantDeafened event: {e}");
                        }
                    },
                    CallSignal::Undeafened => {
                        if let Err(e) = ch.send(BlinkEventKind::ParticipantUndeafened { peer_id: sender }) {
                            log::error!("failed to send ParticipantUndeafened event: {e}");
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

                let signal: PeerSignal = {
                    let lock = own_id.read().await;
                    let own_id = match lock.as_ref().ok_or(Error::BlinkNotInitialized) {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("{e}");
                            continue;
                        }
                    };
                    match decode_gossipsub_msg_ecdh(own_id, &sender, &msg) {
                        Ok(s) => s,
                        Err(e) => {
                            log::error!("failed to decode msg from call signaling stream: {e}");
                            continue;
                        },
                    }
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
                        let own_id = match lock.as_ref().ok_or(Error::BlinkNotInitialized) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("{e}");
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
                                if peer == *own_id {
                                    log::warn!("got TrackAdded event for own id");
                                    continue;
                                }
                                let audio_sink_codec = audio_sink_codec.read().await.clone();
                                if let Err(e) =   host_media::create_audio_sink_track(peer.clone(), event_ch.clone(), track, AudioCodec::default(), audio_sink_codec).await {
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
                                // only autoclose for 2-person calls (group or direct).
                                // library user should respond to CallTerminated event.
                                if all_closed && active_call.call.participants().len() == 2 {
                                    log::info!("all participants have successfully been disconnected");
                                    if let Err(e) = webrtc_controller.deinit().await {
                                        log::error!("webrtc deinit failed: {e}");
                                    }
                                    //rtp_logger::deinit().await;
                                    host_media::reset().await;
                                    let event = BlinkEventKind::CallTerminated { call_id };
                                    let _ = ch.send(event);
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
                                if let Err(e) = send_signal_ecdh(&ipfs, own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Sdp { dest, sdp } => {
                                let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                                let signal = PeerSignal::Sdp(*sdp);
                                if let Err(e) = send_signal_ecdh(&ipfs, own_id, &dest, signal, topic).await {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            EmittedEvents::Ice { dest, candidate } => {
                               let topic = ipfs_routes::peer_signal_route(&dest, &call_id);
                               let signal = PeerSignal::Ice(*candidate);
                                if let Err(e) = send_signal_ecdh(&ipfs, own_id, &dest, signal, topic).await {
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
