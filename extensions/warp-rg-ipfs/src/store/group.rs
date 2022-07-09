use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use futures::StreamExt;
use ipfs::{Ipfs, PeerId, Types};

use serde::{Deserialize, Serialize};
use warp::crypto::curve25519_dalek::traits::Identity;
use warp::crypto::PublicKey;
use warp::error::Error;
use warp::multipass::identity::FriendRequest;
use warp::multipass::MultiPass;
use warp::raygun::group::GroupId;
use warp::raygun::Message;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

pub struct GroupStore {
    ipfs: Ipfs<Types>,

    peer_list: Arc<Mutex<Vec<PublicKey>>>,

    // Request meant for the user
    message: Arc<Mutex<HashMap<GroupId, Message>>>,

    // Account
    account: Arc<Mutex<Box<dyn MultiPass>>>,

    // Sender to thread
    task: Sender<()>,
}
