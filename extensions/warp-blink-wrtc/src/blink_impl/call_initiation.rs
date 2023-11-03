




use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};




use futures::StreamExt;

use rust_ipfs::{SubscriptionStream};

use tokio::{
    sync::{
        broadcast::{Sender},
        RwLock,
    },
};
use uuid::Uuid;
use warp::{
    blink::{BlinkEventKind},
    crypto::{did_key::Generate, DID},
    error::Error,
};

use crate::{
    signaling::{InitiationSignal},
    store::{
        decode_gossipsub_msg_ecdh,
        PeerIdExt,
    },
};

use super::data::PendingCall;

pub async fn run(
    own_id: Arc<RwLock<Option<DID>>>,
    pending_calls: Arc<RwLock<HashMap<Uuid, PendingCall>>>,
    mut stream: SubscriptionStream,
    ch: Sender<BlinkEventKind>,
) {
    while let Some(msg) = stream.next().await {
        let sender = match msg.source.and_then(|s| s.to_did().ok()) {
            Some(id) => id,
            None => {
                log::error!("msg received without source");
                continue;
            }
        };

        let signal: InitiationSignal = {
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
                    log::error!("failed to decode msg from call initiation stream: {e}");
                    continue;
                }
            }
        };

        match signal {
            InitiationSignal::Offer { call_info } => {
                if !call_info.participants().contains(&sender) {
                    log::warn!("someone offered a call for which they weren't a participant");
                    continue;
                }
                let call_id = call_info.call_id();
                let evt = BlinkEventKind::IncomingCall {
                    call_id,
                    conversation_id: call_info.conversation_id(),
                    sender: sender.clone(),
                    participants: call_info.participants(),
                };

                let pc = PendingCall {
                    call: call_info,
                    connected_participants: HashSet::from_iter(vec![sender].drain(..)),
                };
                pending_calls.write().await.insert(call_id, pc);
                if let Err(e) = ch.send(evt) {
                    log::error!("failed to send IncomingCall event: {e}");
                }
            }
            InitiationSignal::Join { call_id } => {
                if let Some(pc) = pending_calls.write().await.get_mut(&call_id) {
                    if !pc.call.participants().contains(&sender) {
                        log::warn!("someone who wasn't a participant tried to cancel the call");
                        continue;
                    }
                    pc.connected_participants.insert(sender);
                }
            }
            InitiationSignal::Leave { call_id } => {
                if let Some(pc) = pending_calls.write().await.get_mut(&call_id) {
                    if !pc.call.participants().contains(&sender) {
                        log::warn!("someone who wasn't a participant tried to cancel the call");
                        continue;
                    }
                    pc.connected_participants.remove(&sender);
                    if pc.connected_participants.is_empty() {
                        let evt = BlinkEventKind::CallCancelled { call_id };
                        if let Err(e) = ch.send(evt) {
                            log::error!("failed to send CallCancelled event: {e}");
                        }
                    }
                }
            }
        }
    }
}
