mod call_initiation;
use call_initiation::run as handle_call_initiation;

mod data;
use data::*;

mod webrtc_handler;
use tokio::sync::Notify;
use webrtc_handler::run as handle_webrtc;
use webrtc_handler::WebRtcHandlerParams;

use anyhow::{bail, Context};
use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use rust_ipfs::{Ipfs, Keypair};
use std::{any::Any, collections::HashMap, str::FromStr, sync::Arc, time::Duration};
use tokio::{
    sync::{
        broadcast::{self},
        RwLock,
    },
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
            AudioCodec, AudioHardwareConfig, AudioSampleRate,
        },
        mp4_logger::Mp4LoggerConfig,
    },
    signaling::{ipfs_routes, CallSignal, InitiationSignal},
    simple_webrtc::{self, events::WebRtcEventStream},
    store::{send_signal_aes, send_signal_ecdh},
};

// implements Blink
#[derive(Clone)]
pub struct BlinkImpl {
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    pending_calls: Arc<RwLock<HashMap<Uuid, PendingCall>>>,
    active_call: Arc<RwLock<Option<ActiveCall>>>,
    webrtc_controller: Arc<RwLock<simple_webrtc::Controller>>,
    // the DID generated from Multipass, never cloned. contains the private key
    own_id: Arc<RwLock<Option<DID>>>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    audio_source_config: Arc<RwLock<AudioHardwareConfig>>,
    audio_sink_config: Arc<RwLock<AudioHardwareConfig>>,

    // subscribes to IPFS topic to receive incoming calls
    offer_handler: Arc<warp::sync::RwLock<JoinHandle<()>>>,
    // handles 3 streams: one for webrtc events and two IPFS topics
    // pertains to the active_call, which is stored in STATIC_DATA
    webrtc_handler: Arc<warp::sync::RwLock<Option<JoinHandle<()>>>>,
    // call initiation signals frequently fail. this will retry while the call is active
    notify: Arc<Notify>,

    // prevents the UI from running multiple tests simultaneously
    audio_device_config: Arc<RwLock<host_media::audio::DeviceConfig>>,
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
            if let Err(e) = webrtc_controller.write().await.deinit().await {
                log::error!("error in webrtc_controller deinit: {e}");
            }
            host_media::audio::automute::stop();
            host_media::reset().await;
            //rtp_logger::deinit().await;
            log::debug!("deinit finished");
        });
    }
}

impl BlinkImpl {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Box<Self>> {
        log::trace!("initializing WebRTC");

        // check SupportedStreamConfigs. if those channels aren't supported, use the default.
        let mut source_config = AudioHardwareConfig {
            sample_rate: AudioSampleRate::High,
            channels: 1,
        };

        let mut sink_config = AudioHardwareConfig {
            sample_rate: AudioSampleRate::High,
            channels: 1,
        };

        let mut selected_speaker = None;
        let mut selected_microphone = None;
        let cpal_host = cpal::default_host();
        if let Some(input_device) = cpal_host.default_input_device() {
            selected_microphone = input_device.name().ok();
            match Self::get_min_source_channels(&input_device) {
                Ok(channels) => {
                    source_config.channels = channels;
                    host_media::change_audio_input(input_device, source_config.clone()).await?;
                }
                Err(e) => log::error!("{e}"),
            }
        } else {
            log::warn!("blink started with no input device");
        }

        if let Some(output_device) = cpal_host.default_output_device() {
            selected_speaker = output_device.name().ok();
            match Self::get_min_sink_channels(&output_device) {
                Ok(channels) => {
                    sink_config.channels = channels;
                    host_media::change_audio_output(output_device, sink_config.clone()).await?;
                }
                Err(e) => log::error!("{e}"),
            }
        } else {
            log::warn!("blink started with no output device");
        }

        let (ui_event_ch, _rx) = broadcast::channel(1024);
        let blink_impl = Self {
            ipfs: Arc::new(RwLock::new(None)),
            pending_calls: Arc::new(RwLock::new(HashMap::new())),
            active_call: Arc::new(RwLock::new(None)),
            webrtc_controller: Arc::new(RwLock::new(simple_webrtc::Controller::new()?)),
            notify: Arc::new(Notify::new()),
            own_id: Arc::new(RwLock::new(None)),
            ui_event_ch,
            audio_source_config: Arc::new(RwLock::new(source_config)),
            audio_sink_config: Arc::new(RwLock::new(sink_config)),
            offer_handler: Arc::new(warp::sync::RwLock::new(tokio::spawn(async {}))),
            webrtc_handler: Arc::new(warp::sync::RwLock::new(None)),
            audio_device_config: Arc::new(RwLock::new(host_media::audio::DeviceConfig::new(
                selected_speaker,
                selected_microphone,
            ))),
        };

        let ipfs = blink_impl.ipfs.clone();
        let own_id = blink_impl.own_id.clone();
        let offer_handler = blink_impl.offer_handler.clone();
        let pending_calls = blink_impl.pending_calls.clone();
        let ui_event_ch = blink_impl.ui_event_ch.clone();

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

                let _own_id = get_keypair_did(_ipfs.keypair()?)?;
                own_id.write().await.replace(_own_id);

                let _offer_handler = tokio::spawn(async move {
                    handle_call_initiation(
                        own_id.clone(),
                        pending_calls,
                        call_offer_stream,
                        ui_event_ch,
                    )
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

        host_media::audio::automute::start();
        Ok(Box::new(blink_impl))
    }

    async fn init_call(&mut self, call: CallInfo) -> Result<(), Error> {
        //rtp_logger::init(call.call_id(), std::path::PathBuf::from("")).await?;
        let lock = self.own_id.read().await;
        let own_id = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;
        let ipfs = self.get_ipfs().await?;

        self.active_call.write().await.replace(call.clone().into());
        let audio_source_config = self.audio_source_config.read().await;
        let webrtc_codec = AudioCodec::default();
        // ensure there is an audio source track
        let rtc_rtp_codec: RTCRtpCodecCapability = RTCRtpCodecCapability {
            mime_type: webrtc_codec.mime_type(),
            clock_rate: webrtc_codec.sample_rate(),
            channels: 1,
            ..Default::default()
        };
        let track = self
            .webrtc_controller
            .write()
            .await
            .add_media_source(host_media::AUDIO_SOURCE_ID.into(), rtc_rtp_codec)
            .await?;
        if let Err(e) = host_media::create_audio_source_track(
            own_id.clone(),
            self.ui_event_ch.clone(),
            track,
            webrtc_codec,
            audio_source_config.clone(),
        )
        .await
        {
            let _ = self
                .webrtc_controller
                .write()
                .await
                .remove_media_source(host_media::AUDIO_SOURCE_ID.into())
                .await;
            return Err(e);
        }

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
        let active_call = self.active_call.clone();
        let webrtc_controller = self.webrtc_controller.clone();
        let audio_sink_config = self.audio_sink_config.clone();
        let ui_event_ch = self.ui_event_ch.clone();
        let event_ch2 = ui_event_ch.clone();

        let webrtc_handle = tokio::task::spawn(async move {
            handle_webrtc(
                WebRtcHandlerParams {
                    own_id,
                    event_ch: event_ch2,
                    ipfs,
                    active_call,
                    webrtc_controller,
                    audio_sink_config,
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

    async fn update_audio_source_config(
        &mut self,
        input_device: &cpal::Device,
    ) -> anyhow::Result<()> {
        let min_channels = Self::get_min_source_channels(input_device)?;
        self.audio_source_config.write().await.channels = min_channels;
        Ok(())
    }

    fn get_min_source_channels(input_device: &cpal::Device) -> anyhow::Result<u16> {
        let min_channels =
            input_device
                .supported_input_configs()?
                .fold(None, |acc: Option<u16>, x| match acc {
                    None => Some(x.channels()),
                    Some(y) => Some(std::cmp::min(x.channels(), y)),
                });
        let channels = min_channels.ok_or(anyhow::anyhow!(
            "unsupported audio input device - no input configuration available"
        ))?;
        Ok(channels)
    }

    async fn update_audio_sink_config(
        &mut self,
        output_device: &cpal::Device,
    ) -> anyhow::Result<()> {
        let min_channels = Self::get_min_sink_channels(output_device)?;
        self.audio_sink_config.write().await.channels = min_channels;
        Ok(())
    }

    fn get_min_sink_channels(output_device: &cpal::Device) -> anyhow::Result<u16> {
        let min_channels =
            output_device
                .supported_output_configs()?
                .fold(None, |acc: Option<u16>, x| match acc {
                    None => Some(x.channels()),
                    Some(y) => Some(std::cmp::min(x.channels(), y)),
                });
        let channels = min_channels.ok_or(anyhow::anyhow!(
            "unsupported audio output device. no output configuration available"
        ))?;
        Ok(channels)
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

        self.update_audio_source_config(&device).await?;
        host_media::change_audio_input(device, self.audio_source_config.read().await.clone())
            .await?;
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

        self.update_audio_sink_config(&device).await?;
        host_media::change_audio_output(device, self.audio_sink_config.read().await.clone())
            .await?;
        Ok(())
    }
}

impl BlinkImpl {
    async fn get_ipfs(&self) -> Result<Ipfs, Error> {
        let lock = self.ipfs.read().await;
        let ipfs = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;
        Ok(ipfs.clone())
    }

    async fn ensure_call_not_in_progress(&self) -> Result<(), Error> {
        if let Some(ac) = self.active_call.read().await.as_ref() {
            if ac.call_state != CallState::Closed {
                return Err(Error::OtherWithContext("previous call not finished".into()));
            }
        }

        Ok(())
    }

    fn reinit_notify(&mut self) {
        self.notify.notify_waiters();
        let new_notify = Arc::new(Notify::new());
        let _to_drop = std::mem::replace(&mut self.notify, new_notify);
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
        self.reinit_notify();
        let ipfs = self.get_ipfs().await?;

        // need to drop lock to self.own_id before calling self.init_call
        {
            let lock = self.own_id.read().await;
            let own_id = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;
            if !participants.contains(own_id) {
                participants.push(DID::from_str(&own_id.fingerprint())?);
            };
        }

        let call_info = CallInfo::new(conversation_id, participants.clone());
        self.init_call(call_info.clone()).await?;

        let call_id = call_info.call_id();
        let notify = self.notify.clone();
        let own_id = self.own_id.clone();
        tokio::task::spawn(async move {
            let handle_signals = async {
                loop {
                    let mut new_participants = vec![];
                    while let Some(dest) = participants.pop() {
                        let lock = own_id.read().await;
                        let own_id = match lock.as_ref() {
                            Some(r) => r,
                            None => {
                                new_participants.push(dest);
                                continue;
                            }
                        };

                        if dest == *own_id {
                            continue;
                        }

                        let topic = ipfs_routes::call_initiation_route(&dest);
                        let signal = InitiationSignal::Offer {
                            call_info: call_info.clone(),
                        };

                        if let Err(e) = send_signal_ecdh(&ipfs, own_id, &dest, signal, topic).await
                        {
                            log::error!("failed to send offer signal: {e}");
                            new_participants.push(dest);
                            continue;
                        }
                    }
                    participants = new_participants;
                    if participants.is_empty() {
                        break;
                    } else {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            };

            tokio::select! {
                _ = handle_signals => {
                    log::debug!("all signals sent successfully");
                },
                _ = notify.notified() => {
                    log::debug!("call retry task successfully cancelled");
                }
            }
        });

        Ok(call_id)
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        self.ensure_call_not_in_progress().await?;
        self.reinit_notify();
        let call = match self.pending_calls.write().await.remove(&call_id) {
            Some(r) => r.call,
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

        let ipfs = self.get_ipfs().await?;
        send_signal_aes(&ipfs, &call.group_key(), signal, topic)
            .await
            .map_err(|e| Error::FailedToSendSignal(e.to_string()))
    }
    /// use the Leave signal as a courtesy, to let the group know not to expect you to join.
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        let ipfs = self.get_ipfs().await?;
        if let Some(pc) = self.pending_calls.write().await.remove(&call_id) {
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Leave { call_id };
            if let Err(e) = send_signal_aes(&ipfs, &pc.call.group_key(), signal, topic).await {
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
        self.reinit_notify();
        let lock = self.own_id.read().await;
        let own_id = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;
        let ipfs = self.get_ipfs().await?;
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
            if let Err(e) = send_signal_aes(&ipfs, &ac.call.group_key(), signal, topic).await {
                log::error!("failed to send signal: {e}");
            } else {
                log::debug!("sent signal to leave call");
            }

            // send extra quit signal
            for participant in ac
                .call
                .participants()
                .iter()
                .filter(|x| !ac.connected_participants.contains_key(x))
            {
                if participant == own_id {
                    continue;
                }
                let topic = ipfs_routes::call_initiation_route(participant);
                let signal = InitiationSignal::Leave {
                    call_id: ac.call.call_id(),
                };
                if let Err(e) = send_signal_ecdh(&ipfs, own_id, participant, signal, topic).await {
                    log::error!("failed to send signal: {e}");
                }
            }

            let r = self.webrtc_controller.write().await.deinit().await;
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
        Box::new(self.audio_device_config.read().await.clone())
    }

    async fn set_audio_device_config(
        &mut self,
        config: Box<dyn AudioDeviceConfig>,
    ) -> Result<(), Error> {
        let audio_device_config = host_media::audio::DeviceConfig::new(
            config.speaker_device_name(),
            config.microphone_device_name(),
        );
        *self.audio_device_config.write().await = audio_device_config;

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

        let lock = self.ipfs.read().await;
        let ipfs = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_muted = true;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Muted;
            if let Err(e) = send_signal_aes(ipfs, &ac.call.group_key(), signal, topic).await {
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

        let lock = self.ipfs.read().await;
        let ipfs = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_muted = false;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Unmuted;
            if let Err(e) = send_signal_aes(ipfs, &ac.call.group_key(), signal, topic).await {
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
        let lock = self.ipfs.read().await;
        let ipfs = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_deafened = true;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Deafened;
            if let Err(e) = send_signal_aes(ipfs, &ac.call.group_key(), signal, topic).await {
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
        let lock = self.ipfs.read().await;
        let ipfs = lock.as_ref().ok_or(Error::BlinkNotInitialized)?;

        if let Some(ac) = self.active_call.write().await.as_mut() {
            ac.call_config.self_deafened = false;
            let call_id = ac.call.call_id();
            let topic = ipfs_routes::call_signal_route(&call_id);
            let signal = CallSignal::Undeafened;
            if let Err(e) = send_signal_aes(ipfs, &ac.call.group_key(), signal, topic).await {
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
