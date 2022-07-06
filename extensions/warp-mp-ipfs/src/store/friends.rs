#![allow(dead_code)]
use std::sync::atomic::AtomicBool;

use ipfs::{Ipfs, PeerId, Types};

use warp::crypto::curve25519_dalek::traits::Identity;
use warp::multipass::identity::FriendRequest;
use warp::sync::{Arc, Mutex};

pub struct FriendsStore {
    ipfs: Ipfs<Types>,
    // In the event we are not connected to a node, this would become helpful in reboadcasting request
    rebroadcast_request: Arc<AtomicBool>,

    // Request meant for the user
    incoming_request: Arc<Mutex<Vec<FriendRequest>>>,

    // Request meant for others
    outgoing_request: Arc<Mutex<Vec<FriendRequest>>>,

    // Reject that been rejected by other users
    rejected_request: Arc<Mutex<Vec<FriendRequest>>>,
}

impl FriendsStore {
    pub fn new(ipfs: Ipfs<Types>) -> Self {
        let rebroadcast_request = Arc::new(AtomicBool::new(false));
        let incoming_request = Arc::new(Mutex::new(Vec::new()));
        let outgoing_request = Arc::new(Mutex::new(Vec::new()));
        let rejected_request = Arc::new(Mutex::new(Vec::new()));
        Self {
            ipfs,
            rebroadcast_request,
            incoming_request,
            outgoing_request,
            rejected_request,
        }
    }
}
