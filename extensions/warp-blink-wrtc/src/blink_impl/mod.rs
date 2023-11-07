mod call_initiation;
use call_initiation::run as handle_call_initiation;

mod data;
use data::*;

mod webrtc_handler;

mod event_handler;
mod gossipsub_listener;
mod gossipsub_sender;

use anyhow::bail;
use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use rust_ipfs::{Ipfs, Keypair};
use std::{any::Any, str::FromStr, sync::Arc, time::Duration};
use tokio::{
    sync::broadcast::{self},
    task::JoinHandle,
};
use uuid::Uuid;
use warp::{
    blink::{AudioDeviceConfig, Blink, BlinkEventKind, BlinkEventStream, CallConfig, CallInfo},
    crypto::{did_key::Generate, zeroize::Zeroizing, DIDKey, Ed25519KeyPair, Fingerprint, DID},
    error::Error,
    module::Module,
    multipass::MultiPass,
    Extension, SingleHandle,
};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

use crate::{
    host_media::{
        self,
        audio::{
            automute::{AutoMuteCmd, AUDIO_CMD_CH},
            AudioCodec,
        },
        mp4_logger::Mp4LoggerConfig,
    },
    signaling::{ipfs_routes, CallSignal, InitiationSignal},
    simple_webrtc::{self},
};

use self::gossipsub_listener::GossipSubListener;
use self::gossipsub_sender::GossipSubSender;

// implements Blink
#[derive(Clone)]
pub struct BlinkImpl {
    // the DID generated from Multipass. has been cloned. doesn't contain the private key anymore.
    own_id: Arc<warp::sync::RwLock<Option<DID>>>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,

    // subscribes to IPFS topic to receive incoming calls
    offer_handler: Arc<warp::sync::RwLock<JoinHandle<()>>>,

    gossipsub_listener: GossipSubListener,
    gossipsub_sender: GossipSubSender,
    event_handler: event_handler::EventHandler,

    drop_handler: Arc<DropHandler>,
}

struct DropHandler {}
impl Drop for DropHandler {
    fn drop(&mut self) {
        tokio::spawn(async move {
            host_media::audio::automute::stop();
            host_media::reset().await;
            log::debug!("blink drop handler finished");
        });
    }
}

impl BlinkImpl {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Box<Self>> {
        log::trace!("initializing WebRTC");

        let mut selected_speaker = None;
        let mut selected_microphone = None;
        let cpal_host = cpal::default_host();
        if let Some(input_device) = cpal_host.default_input_device() {
            selected_microphone = input_device.name().ok();
            host_media::change_audio_input(input_device).await?;
        } else {
            log::warn!("blink started with no input device");
        }

        if let Some(output_device) = cpal_host.default_output_device() {
            selected_speaker = output_device.name().ok();
            host_media::change_audio_output(output_device).await?;
        } else {
            log::warn!("blink started with no output device");
        }

        let ipfs = Arc::new(warp::sync::RwLock::new(None));
        let own_id_private = Arc::new(warp::sync::RwLock::new(None));
        let gossipsub_sender = GossipSubSender::new(own_id_private, ipfs.clone());
        let gossipsub_listener =
            GossipSubListener::new(ipfs.clone(), todo!(), todo!(), gossipsub_sender.clone());

        let webrtc_controller = simple_webrtc::Controller::new()?;
        let webrtc_event_stream = webrtc_controller.get_event_stream();
        let event_handler = event_handler::EventHandler::new(
            webrtc_controller,
            webrtc_event_stream,
            gossipsub_sender.clone(),
            todo!(),
        );

        let (ui_event_ch, _rx) = broadcast::channel(1024);
        let blink_impl = Self {
            own_id: Arc::new(warp::sync::RwLock::new(None)),
            ui_event_ch,
            offer_handler: Arc::new(warp::sync::RwLock::new(tokio::spawn(async {}))),
            gossipsub_sender,
            gossipsub_listener,
            event_handler,
            drop_handler: Arc::new(DropHandler {}),
        };

        let own_id = blink_impl.own_id.clone();
        let offer_handler = blink_impl.offer_handler.clone();
        let ui_event_ch = blink_impl.ui_event_ch.clone();
        let gossipsub_listener = blink_impl.gossipsub_listener.clone();

        tokio::spawn(async move {
            let f = async move {
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

                let _own_id = get_keypair_did(_ipfs.keypair()?)?;
                let public_did = _own_id.clone();
                // this one better not be cloned
                own_id_private.write().replace(_own_id);
                // this one is for blink and can be cloned. might not even be needed.
                own_id.write().replace(public_did.clone());
                ipfs.write().replace(_ipfs);

                gossipsub_listener.receive_calls(public_did);
                log::trace!("finished initializing WebRTC");
                Ok(())
            };

            // todo: put this in a loop?
            if let Err(e) = f.await {
                log::error!("failed to init blink: {e}");
            }
        });

        host_media::audio::automute::start();
        Ok(Box::new(blink_impl))
    }

    async fn init_call(&mut self, call: CallInfo) -> Result<(), Error> {
        //rtp_logger::init(call.call_id(), std::path::PathBuf::from("")).await?;

        let own_id = self
            .own_id
            .read()
            .clone()
            .ok_or(Error::BlinkNotInitialized)?;

        self.event_handler.set_active_call(call.clone());

        let webrtc_codec = AudioCodec::default();
        // ensure there is an audio source track
        let rtc_rtp_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
            mime_type: webrtc_codec.mime_type(),
            clock_rate: webrtc_codec.sample_rate(),
            channels: 1,
            ..Default::default()
        };
        let track = self
            .event_handler
            .add_media_source(host_media::AUDIO_SOURCE_ID.into(), rtc_rtp_codec)
            .await?;
        if let Err(e) = host_media::create_audio_source_track(
            own_id.clone(),
            self.ui_event_ch.clone(),
            track,
            webrtc_codec,
        )
        .await
        {
            let _ = self
                .event_handler
                .remove_media_source(host_media::AUDIO_SOURCE_ID.into());
            return Err(e);
        }

        self.gossipsub_listener
            .subscribe_call(call.call_id(), call.group_key());
        self.gossipsub_listener
            .connect_webrtc(call.call_id(), own_id);

        Ok(())
    }

    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error> {
        let host = cpal::default_host();
        let device: cpal::Device = if device_name.to_ascii_lowercase().eq("default") {
            host.default_input_device()
                .ok_or(Error::AudioDeviceNotFound)?
        } else {
            let mut devices = host
                .input_devices()
                .map_err(|e| Error::AudioHostError(e.to_string()))?;
            let r = devices.find(|x| x.name().map(|name| name == device_name).unwrap_or_default());
            r.ok_or(Error::AudioDeviceNotFound)?
        };

        host_media::change_audio_input(device).await?;
        Ok(())
    }

    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error> {
        let host = cpal::default_host();
        let device: cpal::Device = if device_name.to_ascii_lowercase().eq("default") {
            host.default_output_device()
                .ok_or(Error::AudioDeviceNotFound)?
        } else {
            let mut devices = host
                .output_devices()
                .map_err(|e| Error::AudioHostError(e.to_string()))?;
            let r = devices.find(|x| x.name().map(|name| name == device_name).unwrap_or_default());
            r.ok_or(Error::AudioDeviceNotFound)?
        };

        host_media::change_audio_output(device).await?;
        Ok(())
    }
}

impl BlinkImpl {
    async fn ensure_call_not_in_progress(&self) -> Result<(), Error> {
        if let Some(ac) = self.active_call.read().await.as_ref() {
            if ac.call_state != CallState::Closed {
                return Err(Error::OtherWithContext("previous call not finished".into()));
            }
        }

        Ok(())
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
    /// webrtc_codec.channels will be assumed to be 1.
    async fn offer_call(
        &mut self,
        conversation_id: Option<Uuid>,
        mut participants: Vec<DID>,
    ) -> Result<Uuid, Error> {
        self.ensure_call_not_in_progress().await?;
        let own_id = self
            .own_id
            .read()
            .clone()
            .ok_or(Error::BlinkNotInitialized)?;

        if !participants.contains(&own_id) {
            participants.push(DID::from_str(&own_id.fingerprint())?);
        };

        let call_info = CallInfo::new(conversation_id, participants.clone());
        self.init_call(call_info.clone()).await?;

        // todo: periodically re-send offer signal
        participants.retain(|x| x != &own_id);
        for dest in participants {
            let topic = ipfs_routes::call_initiation_route(&dest);
            let signal = InitiationSignal::Offer {
                call_info: call_info.clone(),
            };

            if let Err(e) = self.gossipsub_sender.send_signal_ecdh(dest, signal, topic) {
                log::error!("failed to send signal: {e}");
            }
        }
        Ok(call_info.call_id())
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        self.ensure_call_not_in_progress().await?;
        let call = match self.pending_calls.write().await.remove(&call_id) {
            Some(r) => r.call,
            None => {
                return Err(Error::OtherWithContext(
                    "could not answer call: not found".into(),
                ))
            }
        };

        self.init_call(call.clone()).await?;

        // todo: periodically re-send join signals
        let call_id = call.call_id();
        let topic = ipfs_routes::call_signal_route(&call_id);
        let signal = CallSignal::Join;
        if let Err(e) = self
            .gossipsub_sender
            .send_signal_aes(call.group_key(), signal, topic)
        {
            log::error!("failed to send signal: {e}");
            Err(Error::OtherWithContext("could not answer call".into()))
        } else {
            Ok(())
        }
    }
    /// use the Leave signal as a courtesy, to let the group know not to expect you to join.
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(pc) = self.pending_calls.write().await.remove(&call_id) {
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave;
            if let Err(e) =
                self.gossipsub_sender
                    .send_signal_aes(pc.call.group_key(), signal, topic)
            {
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
        let own_id = self
            .own_id
            .read()
            .clone()
            .ok_or(Error::BlinkNotInitialized)?;

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
            let signal = CallSignal::Leave;
            if let Err(e) =
                self.gossipsub_sender
                    .send_signal_aes(ac.call.group_key(), signal, topic)
            {
                log::error!("failed to send signal: {e}");
            }

            let r = self.event_handler.leave_call();
            host_media::reset().await;
            //rtp_logger::deinit().await;
            let _ = r?;
            Ok(())
        } else {
            Err(Error::OtherWithContext(
                "tried to leave nonexistent call".into(),
            ))
        }
    }

    // ------ Select input/output devices ------

    async fn get_audio_device_config(&self) -> Box<dyn AudioDeviceConfig> {
        Box::new(host_media::get_audio_device_config().await)
    }

    async fn set_audio_device_config(
        &mut self,
        config: Box<dyn AudioDeviceConfig>,
    ) -> Result<(), Error> {
        if let Some(device_name) = config.speaker_device_name() {
            self.select_speaker(&device_name).await?;
        }
        if let Some(device_name) = config.microphone_device_name() {
            self.select_microphone(&device_name).await?;
        }
        Ok(())
    }

    async fn get_available_cameras(&self) -> Result<Vec<String>, Error> {
        Err(Error::Unimplemented)
    }

    async fn select_camera(&mut self, _device_name: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error> {
        if self.active_call.read().await.is_none() {
            return Err(Error::CallNotInProgress);
        }
        host_media::mute_self().await?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_muted = true;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Muted;
            if let Err(e) =
                self.gossipsub_sender
                    .send_signal_aes(ac.call.group_key(), signal, topic)
            {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to mute self");
            }
        }

        Ok(())
    }
    async fn unmute_self(&mut self) -> Result<(), Error> {
        if self.active_call.read().await.is_none() {
            return Err(Error::CallNotInProgress);
        }
        host_media::unmute_self().await?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_muted = false;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Unmuted;
            if let Err(e) =
                self.gossipsub_sender
                    .send_signal_aes(ac.call.group_key(), signal, topic)
            {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to unmute self");
            }
        }

        Ok(())
    }
    async fn silence_call(&mut self) -> Result<(), Error> {
        if self.active_call.read().await.is_none() {
            return Err(Error::CallNotInProgress);
        }
        host_media::deafen().await?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_deafened = true;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Deafened;
            if let Err(e) =
                self.gossipsub_sender
                    .send_signal_aes(ac.call.group_key(), signal, topic)
            {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to deafen self");
            }
        }

        Ok(())
    }
    async fn unsilence_call(&mut self) -> Result<(), Error> {
        if self.active_call.read().await.is_none() {
            return Err(Error::CallNotInProgress);
        }
        host_media::undeafen().await?;
        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_deafened = false;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Undeafened;
            if let Err(e) =
                self.gossipsub_sender
                    .send_signal_aes(ac.call.group_key(), signal, topic)
            {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to undeafen self");
            }
        }

        Ok(())
    }

    async fn get_call_config(&self) -> Result<Option<CallConfig>, Error> {
        Ok(self
            .active_call
            .read()
            .await
            .as_ref()
            .map(|x| x.call_config.clone()))
    }

    async fn enable_camera(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn disable_camera(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn record_call(&mut self, output_dir: &str) -> Result<(), Error> {
        match self.active_call.read().await.as_ref() {
            None => return Err(Error::CallNotInProgress),
            Some(ActiveCall { call, .. }) => {
                host_media::init_recording(Mp4LoggerConfig {
                    call_id: call.call_id(),
                    participants: call.participants(),
                    audio_codec: AudioCodec::default(),
                    log_path: output_dir.into(),
                })
                .await?;
            }
        }

        Ok(())
    }
    async fn stop_recording(&mut self) -> Result<(), Error> {
        match self.active_call.read().await.as_ref() {
            None => return Err(Error::CallNotInProgress),
            Some(_) => {
                host_media::pause_recording().await?;
            }
        }

        Ok(())
    }

    fn enable_automute(&mut self) -> Result<(), Error> {
        let tx = AUDIO_CMD_CH.tx.clone();
        tx.send(AutoMuteCmd::Enable)
            .map_err(|e| Error::OtherWithContext(format!("failed to enable automute: {e}")))
    }
    fn disable_automute(&mut self) -> Result<(), Error> {
        let tx = AUDIO_CMD_CH.tx.clone();
        tx.send(AutoMuteCmd::Disable)
            .map_err(|e| Error::OtherWithContext(format!("failed to disable automute: {e}")))
    }

    async fn set_peer_audio_gain(&mut self, peer_id: DID, multiplier: f32) -> Result<(), Error> {
        host_media::set_peer_audio_gain(peer_id, multiplier).await?;
        Ok(())
    }

    // ------ Utility Functions ------

    async fn pending_calls(&self) -> Vec<CallInfo> {
        Vec::from_iter(
            self.pending_calls
                .read()
                .await
                .values()
                .map(|x| x.call.clone()),
        )
    }
    async fn current_call(&self) -> Option<CallInfo> {
        self.active_call
            .read()
            .await
            .as_ref()
            .map(|x| x.call.clone())
    }
}

pub fn get_keypair_did(keypair: &Keypair) -> anyhow::Result<DID> {
    let kp = Zeroizing::new(keypair.clone().try_into_ed25519()?.to_bytes());
    let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&*kp)?;
    let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
    Ok(did.into())
}
