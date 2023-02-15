use std::sync::Arc;

use ipfs::{Ipfs, IpfsTypes};
use uuid::Uuid;
use warp::crypto::DID;

struct Call<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    id: Uuid,
    participants: Vec<DID>,
    state: CallState,
}

enum CallState {
    Pending,
    InProgress,
    Ended,
}

impl<T: IpfsTypes> Call<T> {
    fn new(ipfs: Ipfs<T>, participants: Vec<DID>) -> Self {
        Self {
            ipfs,
            id: Uuid::new_v4(),
            state: CallState::Pending,
            participants,
        }
    }
}
