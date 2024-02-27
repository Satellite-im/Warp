mod blink_controller;
mod data;
mod gossipsub_listener;
mod gossipsub_sender;
mod signaling;
mod store;

use anyhow::bail;
use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use rust_ipfs::{Ipfs, Keypair};
use std::{any::Any, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::{
    broadcast::{self},
    mpsc,
};
use uuid::Uuid;
use warp::{
    blink::{AudioDeviceConfig, Blink, BlinkEventKind, BlinkEventStream, CallInfo, CallState},
    crypto::{did_key::Generate, zeroize::Zeroizing, DIDKey, Ed25519KeyPair, Fingerprint, DID},
    error::Error,
    module::Module,
    multipass::MultiPass,
    Extension, SingleHandle,
};

use crate::{
    blink_impl::blink_controller::BlinkController,
    host_media::{self, audio_utils::automute},
    simple_webrtc::{self},
};

use self::gossipsub_listener::GossipSubListener;
use self::gossipsub_sender::GossipSubSender;

// implements Blink
#[derive(Clone)]
pub struct BlinkImpl {
    // the DID generated from Multipass. has been cloned. doesn't contain the private key anymore.
    own_id: Arc<parking_lot::RwLock<Option<DID>>>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,

    gossipsub_listener: GossipSubListener,
    gossipsub_sender: GossipSubSender,
    blink_controller: blink_controller::BlinkController,

    drop_handler: Arc<DropHandler>,
}

struct DropHandler {}
impl Drop for DropHandler {
    fn drop(&mut self) {
        host_media::audio_utils::automute::stop();
        tokio::spawn(async {
            host_media::controller::reset().await;
        });
    }
}

impl BlinkImpl {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Box<Self>> {
        log::trace!("initializing WebRTC");

        let (ui_event_ch, _rx) = broadcast::channel(1024);
        let (gossipsub_tx, gossipsub_rx) = mpsc::unbounded_channel();

        let ipfs = Arc::new(parking_lot::RwLock::new(None));
        let own_id_private = Arc::new(parking_lot::RwLock::new(None));
        let gossipsub_sender = GossipSubSender::new(own_id_private.clone(), ipfs.clone());
        let gossipsub_listener =
            GossipSubListener::new(ipfs.clone(), gossipsub_tx, gossipsub_sender.clone());

        let webrtc_controller = simple_webrtc::Controller::new()?;
        let webrtc_event_stream = webrtc_controller.get_event_stream();
        let blink_controller = BlinkController::new(blink_controller::Args {
            webrtc_controller,
            webrtc_event_stream,
            gossipsub_sender: gossipsub_sender.clone(),
            gossipsub_listener: gossipsub_listener.clone(),
            signal_rx: gossipsub_rx,
            ui_event_ch: ui_event_ch.clone(),
        });

        let blink_impl = Self {
            own_id: Arc::new(parking_lot::RwLock::new(None)),
            ui_event_ch: ui_event_ch.clone(),
            gossipsub_sender,
            gossipsub_listener,
            blink_controller,
            drop_handler: Arc::new(DropHandler {}),
        };

        let own_id = blink_impl.own_id.clone();
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

                let _own_id = get_keypair_did(_ipfs.keypair())?;
                let public_did = identity.did_key();
                // this one better not be cloned
                own_id_private.write().replace(_own_id);
                // this one is for blink and can be cloned. might not even be needed.
                own_id.write().replace(public_did.clone());
                ipfs.write().replace(_ipfs);

                let cpal_host = cpal::default_host();
                if let Some(input_device) = cpal_host.default_input_device() {
                    if let Err(e) = host_media::controller::change_audio_input(
                        &public_did,
                        input_device,
                        ui_event_ch.clone(),
                    )
                    .await
                    {
                        log::error!("BlinkImpl failed to set audio input device: {e}");
                    }
                } else {
                    log::warn!("blink started with no input device");
                }

                if let Some(output_device) = cpal_host.default_output_device() {
                    if let Err(e) = host_media::controller::change_audio_output(output_device).await
                    {
                        log::error!("BlinkImpl failed to set audio output device: {e}");
                    }
                } else {
                    log::warn!("blink started with no output device");
                }

                gossipsub_listener.receive_calls(public_did);
                log::trace!("finished initializing WebRTC");
                Ok(())
            };

            // todo: put this in a loop?
            if let Err(e) = f.await {
                log::error!("failed to init blink: {e}");
            }
        });

        host_media::audio_utils::automute::start();
        Ok(Box::new(blink_impl))
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

        let opt = self.own_id.read().clone();
        match opt {
            Some(id) => {
                host_media::controller::change_audio_input(&id, device, self.ui_event_ch.clone())
                    .await?;
                Ok(())
            }
            None => Err(Error::BlinkNotInitialized),
        }
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

        host_media::controller::change_audio_output(device).await?;
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
        let own_id = self
            .own_id
            .read()
            .clone()
            .ok_or(Error::BlinkNotInitialized)?;

        if !participants.contains(&own_id) {
            participants.push(DID::from_str(&own_id.fingerprint())?);
        };

        let call_info = CallInfo::new(conversation_id, participants);
        let call_id = call_info.call_id();
        self.blink_controller.offer_call(call_info).await?;

        Ok(call_id)
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        self.blink_controller.answer_call(call_id).await
        // todo: periodically re-send join signals
    }
    /// use the Leave signal as a courtesy, to let the group know not to expect you to join.
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        self.blink_controller.leave_call(Some(call_id))?;
        Ok(())
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        self.blink_controller.leave_call(None)?;
        Ok(())
    }

    // ------ Select input/output devices ------

    async fn get_audio_device_config(&self) -> Result<Box<dyn AudioDeviceConfig>, Error> {
        Ok(Box::new(
            host_media::controller::get_audio_device_config().await,
        ))
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
        self.blink_controller.mute_self()?;
        Ok(())
    }
    async fn unmute_self(&mut self) -> Result<(), Error> {
        self.blink_controller.unmute_self()?;
        Ok(())
    }
    async fn silence_call(&mut self) -> Result<(), Error> {
        self.blink_controller.silence_call()?;
        Ok(())
    }
    async fn unsilence_call(&mut self) -> Result<(), Error> {
        self.blink_controller.unsilence_call()?;
        Ok(())
    }

    async fn get_call_state(&self) -> Result<Option<CallState>, Error> {
        self.blink_controller.get_active_call_state().await
    }

    async fn enable_camera(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn disable_camera(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn record_call(&mut self, output_dir: &str) -> Result<(), Error> {
        self.blink_controller.record_call(output_dir.into()).await
    }
    async fn stop_recording(&mut self) -> Result<(), Error> {
        self.blink_controller.stop_recording().await
    }

    fn enable_automute(&mut self) -> Result<(), Error> {
        let tx = automute::AUDIO_CMD_CH.tx.clone();
        tx.send(automute::Cmd::Enable)
            .map_err(|e| Error::OtherWithContext(format!("failed to enable automute: {e}")))
    }
    fn disable_automute(&mut self) -> Result<(), Error> {
        let tx = automute::AUDIO_CMD_CH.tx.clone();
        tx.send(automute::Cmd::Disable)
            .map_err(|e| Error::OtherWithContext(format!("failed to disable automute: {e}")))
    }

    async fn set_peer_audio_gain(&mut self, peer_id: DID, multiplier: f32) -> Result<(), Error> {
        host_media::controller::set_peer_audio_gain(peer_id, multiplier).await;
        Ok(())
    }

    // ------ Utility Functions ------

    async fn pending_calls(&self) -> Vec<CallInfo> {
        match self.blink_controller.get_pending_calls().await {
            Ok(r) => r,
            Err(e) => {
                log::error!("{e}");
                vec![]
            }
        }
    }
    async fn current_call(&self) -> Option<CallInfo> {
        match self.blink_controller.get_active_call_info().await {
            Ok(r) => r,
            Err(e) => {
                log::error!("{e}");
                None
            }
        }
    }
}

pub fn get_keypair_did(keypair: &Keypair) -> anyhow::Result<DID> {
    let kp = Zeroizing::new(keypair.clone().try_into_ed25519()?.to_bytes());
    let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&*kp)?;
    let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
    Ok(did.into())
}
