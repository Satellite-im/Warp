use std::fmt::Display;

use uuid::Uuid;
use warp::crypto::DID;

pub mod protocol;

pub trait ConversationTopic: Display {
    fn base(&self) -> String;
    fn event_topic(&self) -> String {
        format!("{}/events", self.base())
    }
    fn exchange_topic(&self, did: &DID) -> String {
        format!("{}/exchange/{}", self.base(), did)
    }
}

impl ConversationTopic for Uuid {
    fn base(&self) -> String {
        format!("/conversation/{self}")
    }
}
