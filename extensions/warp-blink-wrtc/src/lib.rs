//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;
use ipfs::libp2p::gossipsub::GossipsubMessage;
use ipfs::Ipfs;
use ipfs::IpfsTypes;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use uuid::Uuid;
use warp::libipld;
use warp::multipass::MultiPass;
use warp::sata::Sata;
use warp::sync::RwLock;
use warp::{
    blink::{Blink, BlinkEventStream},
    crypto::DID,
    error::Error,
};
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

mod signaling;
mod simple_webrtc;

mod ipfs_routes {
    use uuid::Uuid;
    use warp::crypto::DID;

    const TELECON_BROADCAST: &str = "telecon";
    const OFFER_CALL: &str = "offer_call";

    pub fn call_broadcast_route(call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}")
    }

    pub fn call_signal_route(peer: &DID, call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}/{peer}")
    }

    pub fn offer_call_route(peer: &DID) -> String {
        format!("{OFFER_CALL}/{peer}")
    }
}

// todo: add option to init WebRtc using a configuration file
pub struct WebRtc<T: IpfsTypes> {
    account: Box<dyn MultiPass>,
    // todo: get this during initialization and don't use an option
    ipfs: Arc<RwLock<Ipfs<T>>>,
    // todo: get this
    id: DID,
    webrtc: Arc<Mutex<simple_webrtc::Controller>>,
    active_call: Option<ActiveCall>,
    pending_calls: HashMap<Uuid, Call>,
    cpal_host: cpal::Host,
    audio_input: Option<cpal::Device>,
    audio_output: Option<cpal::Device>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Call {
    id: Uuid,
    participants: Vec<DID>,
}

#[derive(Clone)]
struct ActiveCall {
    call: Call,
    state: CallState,
    stop: Arc<Notify>,
}

#[derive(Clone)]
enum CallState {
    Pending,
    InProgress,
    Ended,
}

impl Call {
    fn new(participants: Vec<DID>) -> Self {
        Self {
            id: Uuid::new_v4(),
            participants,
        }
    }
}

// used when a call is offered
impl ActiveCall {
    fn new(participants: Vec<DID>) -> Self {
        Self {
            call: Call::new(participants),
            state: CallState::Pending,
            stop: Arc::new(Notify::new()),
        }
    }
}

// used when a call is accepted
impl From<Call> for ActiveCall {
    fn from(value: Call) -> Self {
        Self {
            call: value,
            state: CallState::InProgress,
            stop: Arc::new(Notify::new()),
        }
    }
}

/// sent via offer_call/<DID>
#[derive(Serialize, Deserialize)]
enum InitiationSignal {
    /// invite a peer to join a call
    Offer(Call),
    /// indicate that the peer will not be joining
    Reject(DID),
}

/// sent via telecon/<Uuid>
#[derive(Serialize, Deserialize)]
enum BroadcastSignal {
    /// Sent when a peer joins a call.
    /// Used by the peers to dial each other
    Hello,
    /// sent when a peer leaves the call
    HangUp,
}

/// sent via telecon/<Uuid>/<DID>
#[derive(Serialize, Deserialize)]
enum DirectSignal {
    /// Initiates a WebRTC connection
    Dial(RTCSessionDescription),
    /// Completes WebRTC initiation. next is ICE discovery
    Sdp(RTCSessionDescription),
    /// Send peer your ICE candidates as they are discovered
    Ice(RTCIceCandidate),
}

#[async_trait]
impl<T: IpfsTypes> Blink for WebRtc<T> {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    async fn get_event_stream(&mut self) -> Result<BlinkEventStream, Error> {
        todo!()
    }

    // ------ Create/Join a call ------

    /// attempt to initiate a call. Only one call may be offered at a time.
    /// cannot offer a call if another call is in progress.
    /// During a call, WebRTC connections should only be made to
    /// peers included in the Vec<DID>.
    async fn offer_call(&mut self, participants: Vec<DID>) -> Result<(), Error> {
        if let Some(_call) = self.active_call.as_ref() {
            // todo: end call
        }
        let ac = ActiveCall::new(participants);
        self.active_call = Some(ac.clone());

        self.init_call(ac.call.clone(), ac.stop).await?;

        let ipfs = self.ipfs.read();
        // send message until participants accept or decline.
        let data = Sata::default();
        let payload = ac.call.clone();
        let res = data.encode(
            libipld::IpldCodec::DagJson,
            warp::sata::Kind::Static,
            payload,
        )?;
        let bytes = match serde_cbor::to_vec(&res) {
            Ok(b) => b,
            Err(e) => {
                log::error!("failed to encode Call struct: {e}");
                return Err(Error::SerdeCborError(e));
            }
        };
        for participant in &ac.call.participants {
            if let Err(e) = ipfs
                .pubsub_publish(ipfs_routes::offer_call_route(participant), bytes.clone())
                .await
            {
                log::error!("failed to offer call to participant {participant}: {e}");
            }
            todo!();
        }

        todo!();
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(_call) = self.active_call.as_ref() {
            // todo: end call
        }

        if let Some(call) = self.pending_calls.remove(&call_id) {
            let ac: ActiveCall = call.into();
            self.active_call = Some(ac.clone());
            self.init_call(ac.call, ac.stop).await?;
        }
        todo!()
    }
    /// notify a sender/group that you will not join a call
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(mut _call) = self.pending_calls.remove(&call_id) {
            // todo: signal
        }
        todo!()
    }
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error> {
        if let Some(ac) = self.active_call.take() {
            // todo: leave call
            ac.stop.notify_waiters();

            let mut webrtc = self.webrtc.lock().await;
            webrtc.deinit().await?;

            // todo: remove media streams
        }
        todo!()
    }

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error> {
        let device_iter = match self.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error> {
        let device_iter = match self.cpal_host.input_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };

        let device = match device_iter
            .filter(|d| d.name().unwrap_or_default() == device_name)
            .next()
        {
            Some(d) => d,
            None => return Err(Error::DeviceNotFound),
        };

        self.audio_input = Some(device);
        Ok(())
    }
    async fn get_available_speakers(&self) -> Result<Vec<String>, Error> {
        let device_iter = match self.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };
        Ok(device_iter
            .map(|device| device.name().unwrap_or(String::from("unknown device")))
            .collect())
    }
    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error> {
        let device_iter = match self.cpal_host.output_devices() {
            Ok(iter) => iter,
            Err(e) => return Err(Error::Cpal(e.to_string())),
        };

        let device = match device_iter
            .filter(|d| d.name().unwrap_or_default() == device_name)
            .next()
        {
            Some(d) => d,
            None => return Err(Error::DeviceNotFound),
        };

        self.audio_output = Some(device);
        Ok(())
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

    /// Returns the ID of the current call, or None if
    /// a call is not in progress
    fn get_call_id(&self) -> Option<Uuid> {
        todo!()
    }
}

impl<T: IpfsTypes> WebRtc<T> {
    pub async fn new(account: Box<dyn MultiPass>) -> anyhow::Result<Self> {
        while account.get_own_identity().is_err() {
            tokio::time::sleep(Duration::from_millis(100)).await
        }

        let did = match account.get_own_identity() {
            Ok(ident) => ident.did_key(),
            Err(e) => anyhow::bail!("failed to get identity: {e}"),
        };

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

        let webrtc = Self {
            webrtc: Arc::new(Mutex::new(simple_webrtc::Controller::new(did.clone())?)),
            account,
            ipfs: Arc::new(RwLock::new(ipfs)),
            id: did,
            active_call: None,
            pending_calls: HashMap::new(),
            cpal_host: cpal::default_host(),
            audio_input: None,
            audio_output: None,
        };

        Ok(webrtc)
    }

    async fn init_call(&self, call: Call, stop: Arc<Notify>) -> anyhow::Result<()> {
        let ipfs = self.ipfs.read();

        let call_broadcast_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_broadcast_route(&call.id))
            .await?;

        let call_signaling_stream = ipfs
            .pubsub_subscribe(ipfs_routes::call_signal_route(&self.id, &call.id))
            .await?;

        // this one is already pinned to the heap
        let mut webrtc_event_stream = async {
            let webrtc = self.webrtc.lock().await;
            webrtc.get_event_stream()
        }
        .await?;

        // SimpleWebRTC instance
        let webrtc = self.webrtc.clone();
        tokio::spawn(async move {
            futures::pin_mut!(call_broadcast_stream);
            futures::pin_mut!(call_signaling_stream);

            loop {
                tokio::select! {
                    opt = call_broadcast_stream.next() => {
                        match opt {
                            Some(message) => {
                                if let Err(_e) = decode_broadcast_signal(message).await {
                                    let _webrtc = webrtc.lock().await;
                                    todo!("handle signal");
                                }
                            }
                            None => {
                                break
                            }
                        }
                    }
                    opt = call_signaling_stream.next() => {
                        match opt {
                            Some(_signal) => {
                                let _webrtc = webrtc.lock().await;
                                // todo: dial if needed
                                todo!("handle signal");
                            }
                            None => break
                        }
                    }
                    opt = webrtc_event_stream.next() => {
                        match opt {
                            Some(_event) => {
                                todo!("handle event");
                            }
                            None => todo!()
                        }
                    }
                    _ = stop.notified() => {
                        log::debug!("call termniated via notify()");
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}

async fn decode_broadcast_signal(message: Arc<GossipsubMessage>) -> anyhow::Result<()> {
    let data = serde_cbor::from_slice::<Sata>(&message.data)?;
    let sdp = data.decode::<BroadcastSignal>()?;

    //todo: verify that message sender is in conversation list
    // todo: initiate WebRTC communications if needed

    todo!()
}
