use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

use rust_ipfs::libp2p::{
    core::upgrade::DeniedUpgrade,
    swarm::{
        handler::ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol,
    },
};
use void::Void;

#[allow(clippy::type_complexity)]
#[allow(deprecated)]
#[derive(Default, Debug)]
pub struct Handler {
    events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,
    reserve: bool,
}

impl Handler {
    pub fn new(reserve: bool) -> Self {
        Self {
            reserve,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum In {
    Reserve,
    Release,
}

#[derive(Debug, Clone)]
pub enum Out {
    Reserved,
    Released,
}

impl ConnectionHandler for Handler {
    type FromBehaviour = In;
    type ToBehaviour = Out;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = Void;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn connection_keep_alive(&self) -> bool {
        self.reserve
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            In::Reserve => {
                if self.reserve {
                    return;
                }
                self.reserve = true;
                self.events
                    .push_back(ConnectionHandlerEvent::NotifyBehaviour(Out::Reserved));
            }
            In::Release => {
                if !self.reserve {
                    return;
                }
                self.reserve = false;
                self.events
                    .push_back(ConnectionHandlerEvent::NotifyBehaviour(Out::Released));
            }
        }
    }

    fn on_connection_event(
        &mut self,
        _: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }
        Poll::Pending
    }
}
