//! simple-webrtc
//! This module augments the [webrtc-rs](https://github.com/webrtc-rs/webrtc) library, hopefully
//! simplifying the process of exchanging media with multiple peers simultaneously.
//!
//! this module allows for the exchange of RTP packets. Transforming audio/video into RTP packets
//! is the user's responsibility. `webrtc-rs` provides a `rtp::packetizer` to turn raw samples into
//! RTP packets. `webrtc-rs` also provides a `media::io::sample_builder` to turn received RTP packets
//! into samples. `simple-webrtc` may expose these interfaces later.
//!
//! The `add_media_source` function returns a `TrackLocalWriter`, with which the user will send
//! their RTP packets. Internally, a track is created and added to all existing and future peer
//! connections.. Writing a packet to the `TrackLocalWriter` will cause the packet to be forwarded
//! to all connected peers.
//!
//! WebRTC requires out of band signalling. The `SimpleWebRtc` accepts a callback for transmitting
//! signals which must be forwarded to the specified peer
//!

use anyhow::{bail, Result};
use webrtc::data_channel::data_channel_message::DataChannelMessage;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;
use warp::blink::MimeType;
use warp::crypto::DID;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
pub use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecParameters, RTPCodecType};
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::sdp::extmap::AUDIO_LEVEL_URI;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use self::events::{EmittedEvents, WebRtcEventStream};

// public exports
pub mod events;

mod time_of_flight;

/// simple-webrtc
/// This library augments the [webrtc-rs](https://github.com/webrtc-rs/webrtc) library, hopefully
/// simplifying the process of exchanging media with multiple peers simultaneously.
///
/// this library allows for the exchange of RTP packets. Transforming audio/video into RTP packets
/// is the user's responsibility. `webrtc-rs` provides a `rtp::packetizer` to turn raw samples into
/// RTP packets. `webrtc-rs` also provides a `media::io::sample_builder` to turn received RTP packets
/// into samples. `simple-webrtc` may expose these interfaces later.
///
/// The `add_media_source` function returns a `TrackLocalWriter`, with which the user will send
/// their RTP packets. Internally, a track is created and added to all existing and future peer
/// connections.. Writing a packet to the `TrackLocalWriter` will cause the packet to be forwarded
/// to all connected peers.
///
/// WebRTC requires out of band signalling. The `SimpleWebRtc` accepts a callback for transmitting
/// signals which must be forwarded to the specified peer
///

pub struct Controller {
    api: Option<webrtc::api::API>,
    peers: HashMap<DID, Peer>,
    event_ch: broadcast::Sender<EmittedEvents>,
    media_sources: HashMap<MediaSourceId, Arc<TrackLocalStaticRTP>>,
    tof: time_of_flight::Controller,
}

/// stores a PeerConnection for updating SDP and ICE candidates, adding and removing tracks
/// also stores associated media streams
pub struct Peer {
    pub id: DID,
    pub connection: Arc<RTCPeerConnection>,
    /// webrtc has a remove_track function which requires passing a RTCRtpSender
    /// to a RTCPeerConnection. this is created by add_track, though the user
    /// only receives a TrackWriter
    /// in the future, the RTCRtpSender can be used to have finer control over the stream.
    /// it can do things like pause the stream, without disconnecting it.
    pub rtp_senders: HashMap<MediaSourceId, RtcRtpManager>,
}

pub struct RtcRtpManager {
    _sender: Arc<RTCRtpSender>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for RtcRtpManager {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub type MediaSourceId = String;

/// The following functions are driven by the UI:
/// dial
/// accept_call
/// hang_up
/// add_media_source
/// remove_media_source
///
/// The following functions are driven by signaling
/// recv_ice
/// recv_sdp
impl Controller {
    pub fn new() -> Result<Self> {
        // todo: verify size
        let (event_ch, _rx) = broadcast::channel(1024);
        let tof = time_of_flight::Controller::new(event_ch.clone());
        Ok(Self {
            api: Some(create_api()?),
            peers: HashMap::new(),
            event_ch,
            media_sources: HashMap::new(),
            tof,
        })
    }
    /// Rust doesn't have async drop, so this function should be called when the user is
    /// done with Controller. it will clean up all threads
    pub async fn deinit(&mut self) -> Result<()> {
        for (_id, peer) in self.peers.drain() {
            if let Err(e) = peer.connection.close().await {
                log::error!("failed to close peer connection: {e}");
            }
        }
        self.tof.reset();
        // remove RTP tracks
        self.media_sources.clear();
        if self.peers.is_empty() {
            Ok(())
        } else {
            bail!("peers is not empty after deinit")
        }
    }
    pub fn get_event_stream(&self) -> WebRtcEventStream {
        let mut rx = self.event_ch.subscribe();
        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };
        WebRtcEventStream(Box::pin(stream))
    }

    /// creates a RTCPeerConnection, sets the local SDP object, emits a CallInitiatedEvent,
    /// which contains the SDP object
    /// continues with the following signals: Sdp, CallTerminated, CallRejected
    pub async fn dial(&mut self, peer_id: &DID) -> Result<()> {
        let peer = self.connect(peer_id).await?;
        let local_sdp = peer.connection.create_offer(None).await?;
        // Sets the LocalDescription, and starts our UDP listeners
        // Note: this will start the gathering of ICE candidates
        peer.connection
            .set_local_description(local_sdp.clone())
            .await?;

        self.peers.insert(peer.id.clone(), peer);
        self.event_ch.send(EmittedEvents::CallInitiated {
            dest: peer_id.clone(),
            sdp: Box::new(local_sdp),
        })?;

        Ok(())
    }
    /// adds the remote sdp, sets own sdp, and sends own sdp to remote
    pub async fn accept_call(
        &mut self,
        peer_id: &DID,
        remote_sdp: RTCSessionDescription,
    ) -> Result<()> {
        let peer = self
            .connect(peer_id)
            .await
            .map_err(|e| anyhow::anyhow!(format!("{e}: {}:{}", file!(), line!())))?;
        peer.connection
            .set_remote_description(remote_sdp)
            .await
            .map_err(|e| anyhow::anyhow!(format!("{e}: {}:{}", file!(), line!())))?;

        let answer = peer
            .connection
            .create_answer(None)
            .await
            .map_err(|e| anyhow::anyhow!(format!("{e}: {}:{}", file!(), line!())))?;
        peer.connection
            .set_local_description(answer.clone())
            .await
            .map_err(|e| anyhow::anyhow!(format!("{e}: {}:{}", file!(), line!())))?;

        if self.peers.insert(peer_id.clone(), peer).is_some() {
            log::warn!("overwriting peer connection");
        }

        self.event_ch.send(EmittedEvents::Sdp {
            dest: peer_id.clone(),
            sdp: Box::new(answer),
        })?;

        Ok(())
    }
    /// Terminates a connection
    /// the controlling application should send a HangUp signal to the remote side
    pub async fn hang_up(&mut self, peer_id: &DID) {
        if let Some(peer) = self.peers.remove(peer_id) {
            if let Err(e) = peer.connection.close().await {
                log::error!("failed to close peer connection: {e}");
            }
        } else {
            log::warn!("attempted to remove nonexistent peer");
        }
    }

    pub fn is_connected(&self, peer_id: &DID) -> bool {
        self.peers.contains_key(peer_id)
    }

    /// Spawns a MediaWorker which will receive RTP packets and forward them to all peers
    /// todo: the peers may want to agree on the MimeType
    pub async fn add_media_source(
        &mut self,
        source_id: MediaSourceId,
        codec: RTCRtpCodecCapability,
    ) -> Result<Arc<TrackLocalStaticRTP>> {
        // todo: don't allow adding duplicate source_ids
        let track = Arc::new(TrackLocalStaticRTP::new(
            codec,
            source_id.clone(),
            Uuid::new_v4().to_string(),
        ));
        // save this for later, for when connections are established to new peers
        self.media_sources.insert(source_id.clone(), track.clone());

        for (peer_id, peer) in &mut self.peers {
            match peer.connection.add_track(track.clone()).await {
                Ok(rtp_sender) => {
                    // returns None if the value was newly inserted.
                    if peer.rtp_senders.contains_key(&source_id) {
                        log::error!("duplicate rtp_sender");
                    } else {
                        // Read incoming RTCP packets
                        // Before these packets are returned they are processed by interceptors. For things
                        // like NACK this needs to be called.
                        let sender2 = rtp_sender.clone();
                        let handle = tokio::spawn(async move {
                            let mut rtcp_buf = vec![0u8; 1500];
                            while let Ok((_, _)) = sender2.read(&mut rtcp_buf).await {}
                            log::debug!("terminating rtp_sender thread from add_media_source");
                        });
                        let x = RtcRtpManager {
                            _sender: rtp_sender,
                            handle,
                        };
                        peer.rtp_senders.insert(source_id.clone(), x);
                    }
                }
                Err(e) => {
                    log::error!(
                        "failed to add track for {} to peer {}: {:?}",
                        &source_id,
                        peer_id,
                        e
                    );
                }
            }
        }

        Ok(track)
    }
    /// Removes the media track
    /// ex: stop sharing screen
    /// the user should discard the TrackLocalWriter which they received from add_media_source
    pub async fn remove_media_source(&mut self, source_id: MediaSourceId) -> Result<()> {
        for (peer_id, peer) in &mut self.peers {
            // if source_id isn't found, it will be logged by the next statement
            if let Some(rtp_sender) = peer.rtp_senders.get(&source_id) {
                if let Err(e) = peer.connection.remove_track(&rtp_sender._sender).await {
                    log::error!(
                        "failed to remove track {} for peer {}: {:?}",
                        &source_id,
                        peer_id,
                        e
                    );
                }
            }

            if peer.rtp_senders.remove(&source_id).is_none() {
                log::warn!("media source {} not found for peer {}", &source_id, peer_id);
            }
        }

        if self.media_sources.remove(&source_id).is_none() {
            log::warn!(
                "media source {} not found in self.media_sources",
                &source_id
            );
        }
        Ok(())
    }

    /// receive an ICE candidate from the remote side
    pub async fn recv_ice(&self, peer_id: &DID, candidate: RTCIceCandidate) -> Result<()> {
        if let Some(peer) = self.peers.get(peer_id) {
            let candidate = candidate.to_json()?.candidate;
            peer.connection
                .add_ice_candidate(RTCIceCandidateInit {
                    candidate,
                    ..Default::default()
                })
                .await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }
    /// receive an SDP object from the remote side
    pub async fn recv_sdp(&self, peer_id: &DID, sdp: RTCSessionDescription) -> Result<()> {
        if let Some(peer) = self.peers.get(peer_id) {
            peer.connection.set_remote_description(sdp).await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }

    /// adds a connection. called by dial and accept_call
    /// inserts the connection into self.peers
    /// initializes state to WaitingForSdp
    async fn connect(&mut self, peer_id: &DID) -> Result<Peer> {
        if self.peers.contains_key(peer_id) {
            bail!("peer already connected: {}", peer_id);
        }

        let api = match self.api.as_ref() {
            Some(r) => r,
            None => bail!("webrtc API not creatd yet!"),
        };

        // create ICE gatherer
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec![
                    "stun:stun.l.google.com:19302".into(),
                    "stun:stun1.l.google.com:19302".into(),
                    "stun:stun2.l.google.com:19302".into(),
                    "stun:stun3.l.google.com:19302".into(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create and store a new RTCPeerConnection
        let peer_connection = Arc::new(api.new_peer_connection(config).await?);
        let mut peer = Peer {
            id: peer_id.clone(),
            connection: peer_connection,
            rtp_senders: HashMap::new(),
        };

        // configure callbacks

        let tx = self.event_ch.clone();
        let dest = peer_id.clone();
        peer.connection.on_peer_connection_state_change(Box::new(
            move |c: RTCPeerConnectionState| {
                log::info!(
                    "WebRTC connection state for peer {} has changed {}",
                    &dest,
                    c
                );
                match c {
                    RTCPeerConnectionState::Unspecified => {}
                    RTCPeerConnectionState::New => {}
                    RTCPeerConnectionState::Connecting => {}
                    RTCPeerConnectionState::Connected => {
                        if let Err(e) = tx.send(EmittedEvents::Connected { peer: dest.clone() }) {
                            log::error!("failed to send Connected event for peer {}: {}", &dest, e);
                        }
                    }
                    RTCPeerConnectionState::Disconnected => {
                        // todo: possibly jut remove the track and wait for another track to be added..
                        if let Err(e) = tx.send(EmittedEvents::Disconnected { peer: dest.clone() })
                        {
                            log::error!(
                                "failed to send disconnect event for peer {}: {}",
                                &dest,
                                e
                            );
                        }
                    }
                    RTCPeerConnectionState::Failed => {
                        if let Err(e) =
                            tx.send(EmittedEvents::ConnectionFailed { peer: dest.clone() })
                        {
                            log::error!(
                                "failed to send ConnectionFailed event for peer {}: {}",
                                &dest,
                                e
                            );
                        }
                    }
                    RTCPeerConnectionState::Closed => {
                        if let Err(e) =
                            tx.send(EmittedEvents::ConnectionClosed { peer: dest.clone() })
                        {
                            log::error!(
                                "failed to send ConnectionClosed event for peer {}: {}",
                                &dest,
                                e
                            );
                        }
                    }
                }

                Box::pin(futures::future::ready(()))
            },
        ));

        let tx = self.event_ch.clone();
        let dest = peer_id.clone();
        peer.connection
            .on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
                if let Some(candidate) = c {
                    if let Err(e) = tx.send(EmittedEvents::Ice {
                        dest: dest.clone(),
                        candidate: Box::new(candidate),
                    }) {
                        log::error!("failed to send ice candidate to peer {}: {}", &dest, e);
                    }
                }
                Box::pin(futures::future::ready(()))
            }));

        // Set the handler for ICE connection state
        // This will notify you when the peer has connected/disconnected
        // the next 2 lines is some nonsense to satisfy the (otherwise excellent) rust compiler
        let dest = peer_id.clone();
        peer.connection.on_ice_connection_state_change(Box::new(
            move |connection_state: RTCIceConnectionState| {
                log::info!(
                    "ICE connection state for peer {} has changed {}",
                    &dest,
                    connection_state
                );

                Box::pin(futures::future::ready(()))
            },
        ));

        // store media tracks when created
        // the next 2 lines is some nonsense to satisfy the (otherwise excellent) rust compiler
        let tx = self.event_ch.clone();
        let dest = peer_id.clone();
        peer.connection.on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
                if let Some(track) = track {
                    if let Err(e) = tx.send(EmittedEvents::TrackAdded {
                        peer: dest.clone(),
                        track,
                    }) {
                        log::error!("failed to send track added event for peer {}: {}", &dest, e);
                    }
                }
                Box::pin(futures::future::ready(()))
            },
        ));

        //let event_ch = self.event_ch.clone();
        let tof_ch = self.tof.get_ch();
        let dest = peer_id.clone();
        peer.connection
            .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
                let dest2 = dest.clone();
                let ch = tof_ch.clone();
                d.on_close(Box::new(move || {
                    let _ = ch.send(time_of_flight::Cmd::Remove {
                        peer: dest2.clone(),
                    });
                    Box::pin(futures::future::ready(()))
                }));

                d.on_open(Box::new(move || Box::pin(futures::future::ready(()))));

                let dest2 = dest.clone();
                let ch = tof_ch.clone();
                d.on_message(Box::new(move |msg: DataChannelMessage| {
                    let mut tof = match serde_cbor::from_slice::<time_of_flight::Tof>(&msg.data) {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("deserialize failed for tof: {}", e);
                            return Box::pin(futures::future::ready(()));
                        }
                    };
                    tof.stamp();
                    if tof.ready() {
                        let _ = ch.send(time_of_flight::Cmd::SendComplete {
                            peer: dest2.clone(),
                            msg: tof.clone(),
                        });
                    }
                    if tof.should_send() {
                        let _ = ch.send(time_of_flight::Cmd::Send {
                            peer: dest2.clone(),
                            msg: tof,
                        });
                    }
                    Box::pin(futures::future::ready(()))
                }));

                Box::pin(futures::future::ready(()))
            }));

        // attach all media sources to the peer
        for (source_id, track) in &self.media_sources {
            match peer.connection.add_track(track.clone()).await {
                Ok(rtp_sender) => {
                    // Read incoming RTCP packets
                    // Before these packets are returned they are processed by interceptors. For things
                    // like NACK this needs to be called.
                    let sender2 = rtp_sender.clone();
                    let handle = tokio::spawn(async move {
                        let mut rtcp_buf = vec![0u8; 1500];
                        while let Ok((_, _)) = sender2.read(&mut rtcp_buf).await {}
                        log::debug!("terminating rtp_sender thread from `connect`");
                    });
                    let x = RtcRtpManager {
                        _sender: rtp_sender,
                        handle,
                    };
                    peer.rtp_senders.insert(source_id.clone(), x);
                }
                Err(e) => {
                    log::error!(
                        "failed to add track for {} to peer {}: {:?}",
                        &source_id,
                        &peer_id,
                        e
                    );
                }
            }
        }

        match peer.connection.create_data_channel("rtt", None).await {
            Ok(dc) => {
                self.tof.add_channel(peer.id.clone(), dc);
            }
            Err(e) => {
                log::error!("failed to open datachannel for peer {}: {}", peer_id, e);
            }
        }

        Ok(peer)
    }
}

// todo: try setting useinbandfec=0 instead of 1
// todo: add support for more codecs. perhaps make it configurable
fn create_api() -> Result<webrtc::api::API> {
    let mut media = MediaEngine::default();

    media.register_header_extension(
        webrtc::rtp_transceiver::rtp_codec::RTCRtpHeaderExtensionCapability {
            uri: AUDIO_LEVEL_URI.into(),
        },
        RTPCodecType::Audio,
        Some(RTCRtpTransceiverDirection::Sendrecv),
    )?;

    media.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 48000,
                channels: 1,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    media.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    media.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 24000,
                channels: 1,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 112,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    media.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 24000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 113,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    media.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 8000,
                channels: 1,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 114,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    media.register_codec(
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: 8000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 115,
            ..Default::default()
        },
        RTPCodecType::Audio,
    )?;

    // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
    // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
    // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
    // for each PeerConnection.
    let mut registry = Registry::new();

    // Use the default set of Interceptors
    registry = register_default_interceptors(registry, &mut media)?;

    // Create the API object with the MediaEngine
    Ok(APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .build())
}
