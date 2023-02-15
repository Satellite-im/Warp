//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;
use ipfs::libp2p::gossipsub::GossipsubMessage;
use ipfs::libp2p::simple;
use ipfs::Ipfs;
use ipfs::IpfsTypes;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;
use warp::blink::MediaCodec;
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

// todo: add option to init WebRtc using a configuration file
pub struct WebRtc<T: IpfsTypes> {
    account: Box<dyn MultiPass>,
    ipfs: Arc<RwLock<Option<Ipfs<T>>>>,
    webrtc: Arc<RwLock<simple_webrtc::Controller>>,
    current_call: Option<Call>,
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

struct ActiveCall {
    call: Call,
    state: CallState,
}

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
        if let Some(_call) = self.current_call.as_ref() {
            // todo: end call
        }
        self.current_call = Some(Call::new(participants));

        // todo: signal
        todo!()
    }
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error> {
        if let Some(_call) = self.current_call.as_ref() {
            // todo: end call
        }

        // todo: fix this
        if let Some(mut call) = self.pending_calls.remove(&call_id) {
            self.current_call = Some(call);
        }
        // todo: emit event
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
        if let Some(_call) = self.current_call.take() {
            // todo: leave call
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
    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        let ipfs_handle = match self.account.handle() {
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
            _ => None,
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => {
                anyhow::bail!("Unable to use IPFS Handle");
            }
        };
        *self.ipfs.write() = Some(ipfs);

        // todo: spawn a task to receive messages
        Ok(())
    }

    async fn offer_call(&self, call: Call) -> anyhow::Result<()> {
        // todo: revisit this hacky code
        let lock = self.ipfs.read();
        let ipfs = match lock.as_ref() {
            Some(i) => i,
            None => bail!("no ipfs instance available"),
        };

        // todo: move this somewhere else
        const TELECON_BROADCAST: &str = "telecon";
        let call_broadcast_stream = ipfs
            .pubsub_subscribe(format!("{TELECON_BROADCAST}/{}", &call.id))
            .await?;

        let call_signaling_stream = ipfs
            .pubsub_subscribe(format!(
                "{TELECON_BROADCAST}/{}/{}",
                &call.id,
                self.account.id()
            ))
            .await?;

        let webrtc = self.webrtc.clone();
        tokio::spawn(async move {
            futures::pin_mut!(call_broadcast_stream);
            futures::pin_mut!(call_signaling_stream);

            // todo: add a way to stop the loop
            loop {
                tokio::select! {
                    opt = call_broadcast_stream.next() => {
                        match opt {
                            Some(message) => {
                                if let Err(_e) = decode_broadcast_signal(message).await {
                                    let _webrtc = webrtc.write();
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
                                let _webrtc = webrtc.write();
                                // todo: dial if needed
                                todo!("handle signal");
                            }
                            None => break
                        }
                    }
                }
            }
        });

        // send message until participants accept or decline.
        let data = Sata::default();
        let payload = call.clone();
        let res = data.encode(
            libipld::IpldCodec::DagJson,
            warp::sata::Kind::Static,
            payload,
        )?;
        let bytes = serde_cbor::to_vec(&res)?;
        for participant in call.participants.iter() {
            ipfs.pubsub_publish(format!("offer_call/{}", participant), bytes.clone())
                .await;
            todo!();
        }

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
