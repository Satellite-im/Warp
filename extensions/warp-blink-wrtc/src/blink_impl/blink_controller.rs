use super::{
    data::CallDataMap,
    gossipsub_listener::GossipSubListener,
    gossipsub_sender::GossipSubSender,
    signaling::{self, ipfs_routes, CallSignal, GossipSubSignal, InitiationSignal, PeerSignal},
};
use crate::{
    host_media::{self, Mp4LoggerConfig, AUDIO_SOURCE_ID},
    notify_wrapper::NotifyWrapper,
    simple_webrtc::{self, events::WebRtcEventStream, MediaSourceId},
};
use futures::channel::oneshot;
use futures::StreamExt;
use std::{cmp, sync::Arc, time::Duration};
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
    blink::{BlinkEventKind, CallInfo, CallState, MimeType},
    error::Error,
};
use webrtc::{
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

#[derive(Debug)]
enum Cmd {
    OfferCall {
        call_info: CallInfo,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    AnswerCall {
        call_id: Uuid,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    AddMediaSource {
        source_id: MediaSourceId,
        codec: RTCRtpCodecCapability,
        rsp: oneshot::Sender<anyhow::Result<Arc<TrackLocalStaticRTP>>>,
    },
    RemoveMediaSource {
        source_id: MediaSourceId,
    },
    GetCallInfo {
        call_id: Uuid,
        rsp: oneshot::Sender<Option<CallInfo>>,
    },
    LeaveCall {
        call_id: Option<Uuid>,
    },
    MuteSelf,
    UnmuteSelf,
    SilenceCall,
    UnsilenceCall,
    GetPendingCalls {
        rsp: oneshot::Sender<Vec<CallInfo>>,
    },
    GetActiveCallInfo {
        rsp: oneshot::Sender<Option<CallInfo>>,
    },
    GetActiveCallState {
        rsp: oneshot::Sender<Option<CallState>>,
    },
    RecordCall {
        output_dir: String,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    StopRecording {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
}

#[derive(Clone)]
pub struct BlinkController {
    ch: UnboundedSender<Cmd>,
    notify: Arc<NotifyWrapper>,
}

pub struct Args {
    pub webrtc_controller: simple_webrtc::Controller,
    pub webrtc_event_stream: WebRtcEventStream,
    pub gossipsub_sender: GossipSubSender,
    pub gossipsub_listener: GossipSubListener,
    pub signal_rx: UnboundedReceiver<GossipSubSignal>,
    pub ui_event_ch: broadcast::Sender<BlinkEventKind>,
}

impl BlinkController {
    pub fn new(args: Args) -> Self {
        let (tx, cmd_rx) = mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let notify2 = notify.clone();
        tokio::spawn(async move {
            run(args, cmd_rx, notify2).await;
        });
        Self {
            ch: tx,
            notify: Arc::new(NotifyWrapper { notify }),
        }
    }

    pub async fn offer_call(&self, call_info: CallInfo) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::OfferCall { call_info, rsp: tx })?;
        rx.await??;
        Ok(())
    }

    pub async fn answer_call(&self, call_id: Uuid) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch
            .send(Cmd::AnswerCall { call_id, rsp: tx })
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        rx.await
            .map_err(|x| Error::FailedToSendSignal(x.to_string()))?
    }

    pub async fn add_media_source(
        &self,
        source_id: MediaSourceId,
        codec: RTCRtpCodecCapability,
    ) -> anyhow::Result<Arc<TrackLocalStaticRTP>> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::AddMediaSource {
            source_id,
            codec,
            rsp: tx,
        })?;
        rx.await?
    }

    pub fn remove_media_source(&self, source_id: MediaSourceId) -> anyhow::Result<()> {
        self.ch.send(Cmd::RemoveMediaSource { source_id })?;
        Ok(())
    }

    pub async fn get_call_info(&self, call_id: Uuid) -> Option<CallInfo> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::GetCallInfo { call_id, rsp: tx }).ok()?;
        rx.await.ok()?
    }

    pub fn leave_call(&self, call_id: Option<Uuid>) -> anyhow::Result<()> {
        self.ch.send(Cmd::LeaveCall { call_id })?;
        Ok(())
    }

    pub fn mute_self(&self) -> anyhow::Result<()> {
        self.ch.send(Cmd::MuteSelf)?;
        Ok(())
    }

    pub fn unmute_self(&self) -> anyhow::Result<()> {
        self.ch.send(Cmd::UnmuteSelf)?;
        Ok(())
    }
    pub fn silence_call(&self) -> anyhow::Result<()> {
        self.ch.send(Cmd::SilenceCall)?;
        Ok(())
    }
    pub fn unsilence_call(&self) -> anyhow::Result<()> {
        self.ch.send(Cmd::UnsilenceCall)?;
        Ok(())
    }

    pub async fn get_pending_calls(&self) -> Result<Vec<CallInfo>, Error> {
        let (tx, rx) = oneshot::channel();
        self.ch
            .send(Cmd::GetPendingCalls { rsp: tx })
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        rx.await.map_err(|x| Error::OtherWithContext(x.to_string()))
    }

    pub async fn get_active_call_info(&self) -> Result<Option<CallInfo>, Error> {
        let (tx, rx) = oneshot::channel();
        self.ch
            .send(Cmd::GetActiveCallInfo { rsp: tx })
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        rx.await.map_err(|x| Error::OtherWithContext(x.to_string()))
    }

    pub async fn get_active_call_state(&self) -> Result<Option<CallState>, Error> {
        let (tx, rx) = oneshot::channel();
        self.ch
            .send(Cmd::GetActiveCallState { rsp: tx })
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        rx.await.map_err(|x| Error::OtherWithContext(x.to_string()))
    }
    pub async fn record_call(&self, output_dir: String) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch
            .send(Cmd::RecordCall {
                output_dir,
                rsp: tx,
            })
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        rx.await
            .map_err(|x| Error::OtherWithContext(x.to_string()))?
    }

    pub async fn stop_recording(&self) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch
            .send(Cmd::StopRecording { rsp: tx })
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        rx.await
            .map_err(|x| Error::OtherWithContext(x.to_string()))?
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(args: Args, mut cmd_rx: UnboundedReceiver<Cmd>, notify: Arc<Notify>) {
    let Args {
        mut webrtc_controller,
        mut webrtc_event_stream,
        gossipsub_sender,
        gossipsub_listener,
        mut signal_rx,
        ui_event_ch,
    } = args;

    let own_id = match gossipsub_sender.get_own_id().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("failed to get own id. quitting blink controller: {e}");
            return;
        }
    };

    // prevent accidental moves
    let own_id = &own_id;
    let own_id_str = own_id.to_string();

    let mut call_data_map = CallDataMap::new(own_id.clone());
    let mut dial_timer = tokio::time::interval_at(
        Instant::now() + Duration::from_millis(3000),
        Duration::from_millis(3000),
    );

    loop {
        tokio::select! {
            _ = notify.notified() => {
                log::debug!("quitting blink event handler");
                break;
            },
            _ = dial_timer.tick() => {
                //log::trace!("dial timer: tick");
                if let Some(data) = call_data_map.get_active() {
                    let call_id = data.info.call_id();
                    for (peer_id, _) in data.state.participants_joined.iter() {
                        if peer_id == own_id {
                            continue;
                        }
                        if webrtc_controller.is_connected(peer_id) {
                            continue;
                        }
                        let peer_str = peer_id.to_string();
                        let mut should_dial = false;
                        for (l, r) in std::iter::zip(peer_str.as_bytes(), own_id_str.as_bytes()) {
                            match l.cmp(r) {
                                cmp::Ordering::Less => {
                                    should_dial = true;
                                    break;
                                }
                                cmp::Ordering::Greater => {
                                    break;
                                }
                                _ => {}
                            }
                        }
                        if should_dial {
                            if let Err(e) = webrtc_controller.dial(peer_id).await {
                                log::error!("failed to dial peer: {e}");
                                continue;
                            }
                            if let Err(e) = ui_event_ch.send(BlinkEventKind::ParticipantJoined { call_id, peer_id: peer_id.clone() }) {
                                log::error!("failed to send ParticipantJoined Event: {e}");
                            }
                        }
                    }
                }
            }
            opt = cmd_rx.recv() => {
                let cmd = match opt {
                    Some(r) => r,
                    None => {
                        log::debug!("blink handler cmd_rx channel is closed. quitting");
                        break;
                    }
                };
                match cmd {
                    Cmd::OfferCall { call_info, rsp } => {
                        if call_data_map.is_active_call(call_info.call_id()) {
                            log::debug!("tried to offer call which is already in progress");
                            let _ = rsp.send(Err(Error::CallAlreadyInProgress));
                            continue;
                        }
                        if let Some(data) = call_data_map.get_active_mut() {
                            data.state.reset_self();
                            let call_id = data.info.call_id();
                            let _ = ui_event_ch.send(BlinkEventKind::CallTerminated { call_id});
                            let _ = webrtc_controller.deinit().await;
                            host_media::controller::reset().await;
                        }
                        let call_id = call_info.call_id();
                        call_data_map.add_call(call_info.clone(), own_id);
                        call_data_map.set_active(call_id);

                        // automatically add an audio track
                        let rtc_rtp_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
                            mime_type: MimeType::OPUS.to_string(),
                            clock_rate: 48000,
                            channels: 1,
                            ..Default::default()
                        };
                        match webrtc_controller.add_media_source(AUDIO_SOURCE_ID.into(), rtc_rtp_codec).await {
                            Ok(track) => {
                                match host_media::controller::create_audio_source_track(
                                    own_id,
                                    ui_event_ch.clone(),
                                    track).await
                                {
                                    Ok(_) => {
                                        log::debug!("sending offer signal");
                                        let call_id = call_info.call_id();
                                        gossipsub_listener
                                            .subscribe_call(call_id, call_info.group_key());
                                        gossipsub_listener
                                            .subscribe_webrtc(call_id, own_id.clone());

                                        let mut participants = call_info.participants();
                                        participants.retain(|x| x != own_id);
                                        for dest in participants {
                                            let topic = ipfs_routes::call_initiation_route(&dest);
                                            let signal = InitiationSignal::Offer {
                                                call_info: call_info.clone(),
                                            };

                                            if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                                                log::error!("failed to send signal: {e}");
                                            }
                                        }

                                        let own_state = call_data_map.get_own_state().unwrap_or_default();
                                        let topic = ipfs_routes::call_signal_route(&call_id);
                                        let signal = CallSignal::Announce { participant_state: own_state };
                                        if let Err(e) =
                                            gossipsub_sender.announce(call_info.group_key(), signal, topic)
                                        {
                                            log::error!("failed to send announce signal: {e}");
                                        }
                                        let _ = rsp.send(Ok(()));
                                    }
                                    Err(e) => {
                                        let _ = webrtc_controller.remove_media_source(AUDIO_SOURCE_ID.into()).await;
                                        let _ = rsp.send(Err(e));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = rsp.send(Err(Error::OtherWithContext(e.to_string())));
                            }
                        }
                    },
                    Cmd::AnswerCall { call_id, rsp } => {
                        if call_data_map.is_active_call(call_id) {
                            log::debug!("tried to answer call which is already in progress");
                            let _ = rsp.send(Err(Error::CallAlreadyInProgress));
                            continue;
                        }

                        let call_info = match call_data_map.get_call_info(call_id) {
                            Some(r) => r,
                            None => {
                                let _ = rsp.send(Err(Error::CallNotFound));
                                continue;
                            }
                        };

                        if let Some(data) = call_data_map.get_active_mut() {
                            data.state.reset_self();
                            let _ = ui_event_ch.send(BlinkEventKind::CallTerminated { call_id: data.info.call_id() });
                            let _ = webrtc_controller.deinit().await;
                            host_media::controller::reset().await;
                        }

                        call_data_map.set_active(call_id);

                        // automatically add an audio track
                        let rtc_rtp_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
                            mime_type: MimeType::OPUS.to_string(),
                            clock_rate: 48000,
                            channels: 1,
                            ..Default::default()
                        };
                        match webrtc_controller.add_media_source(AUDIO_SOURCE_ID.into(), rtc_rtp_codec).await {
                            Ok(track) => {
                                let r = host_media::controller::create_audio_source_track(
                                    own_id,
                                    ui_event_ch.clone(),
                                    track).await;
                                match r {
                                    Ok(_) => {
                                        log::debug!("answering call");
                                        gossipsub_listener.subscribe_call(call_id, call_info.group_key());
                                        gossipsub_listener.subscribe_webrtc(call_id, own_id.clone());

                                        let own_state = call_data_map.get_own_state().unwrap_or_default();
                                        let topic = ipfs_routes::call_signal_route(&call_id);
                                        let signal = CallSignal::Announce { participant_state: own_state };
                                        if let Err(e) =
                                            gossipsub_sender.announce(call_info.group_key(), signal, topic)
                                        {
                                            let _ = rsp.send(Err(Error::FailedToSendSignal(e.to_string())));
                                        } else {
                                            let _ = rsp.send(Ok(()));
                                        }
                                    }
                                    Err(e) => {
                                        let _ = webrtc_controller.remove_media_source(AUDIO_SOURCE_ID.into()).await;
                                        let _ = rsp.send(Err(e));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = rsp.send(Err(e.into()));
                            }
                        }
                    }
                    Cmd::AddMediaSource { source_id, codec, rsp } => {
                        let r = webrtc_controller.add_media_source(source_id, codec).await;
                        let _ = rsp.send(r);
                    },
                    Cmd::GetCallInfo { call_id, rsp } => {
                        let _ = rsp.send(call_data_map.get_call_info(call_id));
                    }
                    Cmd::RemoveMediaSource { source_id } => {
                        let _ = webrtc_controller.remove_media_source(source_id).await;
                    },
                    Cmd::LeaveCall { call_id } => {
                        let call_id = call_id.unwrap_or(call_data_map.active_call.unwrap_or_default());
                        if call_data_map.is_active_call(call_id) {
                            call_data_map.leave_call(call_id);
                            let _ = gossipsub_sender.empty_queue();
                            let _ = webrtc_controller.deinit().await;
                            host_media::controller::reset().await;
                            if let Err(e) = ui_event_ch.send(BlinkEventKind::CallTerminated { call_id }) {
                                log::error!("failed to send CallTerminated Event: {e}");
                            }
                        }

                        // todo: if someone tries to dial you when you left the call, resend the leave signal
                        match call_data_map.get_call_info(call_id) {
                            Some(info) => {
                                let topic = ipfs_routes::call_signal_route(&call_id);
                                let signal = CallSignal::Leave;
                                if let Err(e) = gossipsub_sender
                                    .send_signal_aes(info.group_key(), signal, topic)
                                {
                                    log::error!("failed to send signal: {e}");
                                }
                            }
                            None => {
                                log::error!("failed to leave call - not found");
                            }
                        }
                    },
                    Cmd::MuteSelf => {
                        if let Some(data) = call_data_map.get_active_mut() {
                            host_media::controller::mute_self().await;
                            let call_id = data.info.call_id();
                            data.state.set_self_muted(true);
                            let own_state = data.get_participant_state(own_id).unwrap_or_default();
                            let topic = ipfs_routes::call_signal_route(&call_id);
                            let signal = CallSignal::Announce { participant_state: own_state };
                            if let Err(e) =
                                gossipsub_sender.announce(data.info.group_key(), signal, topic)
                            {
                                log::error!("failed to send announce signal: {e}");
                            }
                        }
                    }
                    Cmd::UnmuteSelf => {
                        if let Some(data) = call_data_map.get_active_mut() {
                            host_media::controller::unmute_self().await;
                            let call_id = data.info.call_id();
                            data.state.set_self_muted(false);
                            let own_state = data.get_participant_state(own_id).unwrap_or_default();
                            let topic = ipfs_routes::call_signal_route(&call_id);
                            let signal = CallSignal::Announce { participant_state: own_state };
                            if let Err(e) =
                                gossipsub_sender.announce(data.info.group_key(), signal, topic)
                            {
                                log::error!("failed to send announce signal: {e}");
                            }
                        }
                    }
                    Cmd::SilenceCall => {
                        if let Some(data) = call_data_map.get_active_mut() {
                            let call_id = data.info.call_id();
                            host_media::controller::deafen().await;
                            data.state.set_deafened(own_id, true);
                            let own_state = data.get_participant_state(own_id).unwrap_or_default();
                            let topic = ipfs_routes::call_signal_route(&call_id);
                            let signal = CallSignal::Announce { participant_state: own_state };
                            if let Err(e) =
                                gossipsub_sender.announce(data.info.group_key(), signal, topic)
                            {
                                log::error!("failed to send announce signal: {e}");
                            }
                        }
                    }
                    Cmd::UnsilenceCall => {
                        if let Some(data) = call_data_map.get_active_mut() {
                            let call_id = data.info.call_id();
                            host_media::controller::undeafen().await;
                            data.state.set_deafened(own_id, false);
                            let own_state = data.get_participant_state(own_id).unwrap_or_default();
                            let topic = ipfs_routes::call_signal_route(&call_id);
                            let signal = CallSignal::Announce { participant_state: own_state };
                            if let Err(e) =
                                gossipsub_sender.announce(data.info.group_key(), signal, topic)
                            {
                                log::error!("failed to send announce signal: {e}");
                            }
                        }
                    }
                    Cmd::GetPendingCalls { rsp } => {
                        let _ = rsp.send(call_data_map.get_pending_calls());
                    }
                    Cmd::GetActiveCallState { rsp } => {
                        let _ = rsp.send(call_data_map.get_active().map(|data| data.get_state()));
                    }
                    Cmd::GetActiveCallInfo { rsp } => {
                        let _ = rsp.send(call_data_map.get_active().map(|data| data.get_info()));
                    }
                    Cmd::RecordCall { output_dir, rsp } => {
                        if let Some(data) = call_data_map.get_active_mut() {
                            let info = data.get_info();
                            match
                            host_media::controller::init_recording(Mp4LoggerConfig {
                                    own_id: own_id.clone(),
                                    call_id: info.call_id(),
                                    participants: info.participants(),
                                    log_path: output_dir.into(),
                                })
                                .await
                            {
                                Ok(_) => {
                                    data.state.set_self_recording(true);
                                    let own_state = data.get_participant_state(own_id).unwrap_or_default();
                                    let topic = ipfs_routes::call_signal_route(&info.call_id());
                                    let signal = CallSignal::Announce { participant_state: own_state };
                                    if let Err(e) =
                                        gossipsub_sender.announce(data.info.group_key(), signal, topic)
                                    {
                                        log::error!("failed to send announce signal: {e}");
                                    }
                                    let _ = rsp.send(Ok(()));
                                }
                                Err(e) => {
                                    let _ = rsp.send(Err(Error::OtherWithContext(e.to_string())));
                                }
                            }
                        } else {
                            let _ = rsp.send(Err(Error::CallNotInProgress));
                        }
                    }
                    Cmd::StopRecording { rsp } => {
                        if let Some(data) = call_data_map.get_active_mut() {
                            host_media::controller::pause_recording().await;
                            data.state.set_self_recording(false);
                            let own_state = data.get_participant_state(own_id).unwrap_or_default();
                            let topic = ipfs_routes::call_signal_route(&data.info.call_id());
                            let signal = CallSignal::Announce { participant_state: own_state };
                            if let Err(e) =
                                gossipsub_sender.announce(data.info.group_key(), signal, topic)
                            {
                                log::error!("failed to send announce signal: {e}");
                            }
                            let _ = rsp.send(Ok(()));
                        } else {
                            let _ = rsp.send(Err(Error::CallNotInProgress));
                        }
                    }
                }
            },
            opt = signal_rx.recv() => {
                let signal = match opt {
                    Some(r) => r,
                    None => {
                        log::debug!("blink handler signal_rx channel is closed. quitting");
                        break;
                    }
                };
                match signal {
                    GossipSubSignal::Peer { sender, call_id, signal } => match *signal {
                        _ if !call_data_map.is_active_call(call_id) => {
                            log::debug!("received webrtc signal for non-active call");
                            continue;
                        }
                        _ if !call_data_map.contains_participant(call_id, &sender) => {
                            log::debug!("received signal from someone who isn't part of the call");
                            continue;
                        }
                        signaling::PeerSignal::Ice(ice) => {
                            if let Err(e) = webrtc_controller.recv_ice(&sender, ice).await {
                                log::error!("failed to recv_ice {}", e);
                            }
                        },
                        signaling::PeerSignal::Sdp(sdp) => {
                            log::debug!("received signal: SDP");
                            if let Err(e) = webrtc_controller.recv_sdp(&sender, sdp).await {
                                log::error!("failed to recv_sdp: {}", e);
                            }
                        },
                        signaling::PeerSignal::Dial(sdp) => {
                            log::debug!("received signal: Dial");
                            // emits the SDP Event, which is sent to the peer via the SDP signal
                            if let Err(e) = webrtc_controller.accept_call(&sender, sdp).await {
                                log::error!("failed to accept_call: {}", e);
                            }
                        },
                    },
                    GossipSubSignal::Call { sender, call_id, signal } => match signal {
                        _ if !call_data_map.contains_participant(call_id, &sender) => {
                            log::debug!("received signal from someone who isn't part of the call");
                            continue;
                        }
                        signaling::CallSignal::Announce { participant_state } => {
                            //log::trace!("received announce from {}", &sender);
                            let prev_state = call_data_map.get_participant_state(call_id, &sender);
                            let state_changed = prev_state.as_ref().map(|x| x != &participant_state).unwrap_or(true);
                            call_data_map.add_participant(call_id, &sender, participant_state.clone());
                            if state_changed {
                                let _ = ui_event_ch.send(BlinkEventKind::ParticipantStateChanged { peer_id: sender, state: participant_state });
                            }
                        },
                        signaling::CallSignal::Leave => {
                            call_data_map.remove_participant(call_id, &sender);
                            let is_call_empty = call_data_map.call_empty(call_id);

                            if call_data_map.is_active_call(call_id) {
                                webrtc_controller.hang_up(&sender).await;
                                if let Err(e) = ui_event_ch.send(BlinkEventKind::ParticipantLeft { call_id, peer_id: sender }) {
                                    log::error!("failed to send ParticipantLeft event: {e}");
                                }
                            } else if is_call_empty {
                                call_data_map.remove_call(call_id);
                                gossipsub_listener.unsubscribe_call(call_id);
                                if let Err(e) = ui_event_ch.send(BlinkEventKind::CallCancelled { call_id }) {
                                    log::error!("failed to send CallCancelled event: {e}");
                                }
                            }
                        },
                    },
                    GossipSubSignal::Initiation { sender, signal } => match signal {
                        signaling::InitiationSignal::Offer { call_info } => {
                            let call_id = call_info.call_id();
                            let conversation_id = call_info.conversation_id();
                            let participants = call_info.participants();
                            call_data_map.add_call(call_info, &sender);

                            if let Err(e) = ui_event_ch.send(BlinkEventKind::IncomingCall { call_id, conversation_id, sender, participants }) {
                                log::error!("failed to send IncomingCall event: {e}");
                            }
                        },
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
                    simple_webrtc::events::EmittedEvents::AudioDegradation { peer } => {
                        let _ = ui_event_ch.send(BlinkEventKind::AudioDegradation { peer_id: peer });
                    }
                    simple_webrtc::events::EmittedEvents::Ice { dest, candidate } => {
                        if let Some(data) = call_data_map.get_active() {
                              let topic = ipfs_routes::peer_signal_route(&dest, &data.info.call_id());
                            let signal = PeerSignal::Ice(*candidate);
                            if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                                log::error!("failed to send signal: {e}");
                            }
                        } else {
                            log::warn!("received EmittedEvents::Ice without active call");
                        }
                    },
                    simple_webrtc::events::EmittedEvents::Connected { peer } => {
                        if let Some(data) = call_data_map.get_active() {
                            let call_id = data.info.call_id();
                            if !call_data_map.contains_participant(call_id, &peer) {
                                log::warn!("webrtc controller connected to a peer who wasn't in the list for the active call");
                               webrtc_controller.hang_up(&peer).await;
                           }
                      } else {
                          log::warn!("received EmittedEvents::Connected without active call");
                      }
                    },
                    simple_webrtc::events::EmittedEvents::Disconnected { peer }
                    | simple_webrtc::events::EmittedEvents::ConnectionFailed { peer }
                    | simple_webrtc::events::EmittedEvents::ConnectionClosed { peer } => {
                        log::debug!("webrtc: closed, disconnected or connection failed");

                        webrtc_controller.hang_up(&peer).await;
                        if let Err(e) = host_media::controller::remove_sink_track(peer.clone()).await {
                            log::error!("failed to remove sink track for peer {peer}: {e}");
                        }

                        if let Some(data) = call_data_map.get_active_mut() {
                            let call_id = data.info.call_id();
                            if data.info.contains_participant(&peer) {
                                data.state.remove_participant(&peer);
                            }
                            if data.info.participants().len() == 2 && data.state.participants_joined.len() <= 1 {
                                log::info!("all participants have successfully been disconnected");
                                if let Err(e) = webrtc_controller.deinit().await {
                                    log::error!("webrtc deinit failed: {e}");
                                }
                                //rtp_logger::deinit().await;
                                host_media::controller::reset().await;
                                let event = BlinkEventKind::CallTerminated { call_id };
                                let _ = ui_event_ch.send(event);

                                gossipsub_listener.unsubscribe_call(call_id);
                                gossipsub_listener.unsubscribe_webrtc(call_id);
                                let _ = gossipsub_sender.empty_queue();
                            }
                        }
                    },
                    simple_webrtc::events::EmittedEvents::Sdp { dest, sdp } => {
                        if let Some(data) = call_data_map.get_active() {
                            let topic = ipfs_routes::peer_signal_route(&dest, &data.info.call_id());
                            let signal = PeerSignal::Sdp(*sdp);
                            if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                                log::error!("failed to send signal: {e}");
                            }
                        } else {
                            log::warn!("received EmittedEvents::Sdp without active call");
                        }
                    },
                    simple_webrtc::events::EmittedEvents::CallInitiated { dest, sdp } => {
                        if let Some(data) = call_data_map.get_active() {
                            log::debug!("sending dial signal");
                            let topic = ipfs_routes::peer_signal_route(&dest, &data.info.call_id());
                            let signal = PeerSignal::Dial(*sdp);
                            if let Err(e) = gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                                log::error!("failed to send signal: {e}");
                            }
                        } else {
                            log::warn!("dialing without active call");
                        }
                    },
                    simple_webrtc::events::EmittedEvents::TrackAdded { peer, track } => {
                        if let Err(e) =   host_media::controller::create_audio_sink_track(peer.clone(), ui_event_ch.clone(), track).await {
                            log::error!("failed to send media_track command: {e}");
                        }
                    },
                }
            }
        }
    }
}
