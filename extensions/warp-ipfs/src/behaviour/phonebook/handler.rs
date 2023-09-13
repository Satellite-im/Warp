use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

use rust_ipfs::libp2p::{
    core::upgrade::DeniedUpgrade,
    swarm::{
        handler::ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent, KeepAlive,
        SubstreamProtocol,
    },
};
use void::Void;

#[allow(clippy::type_complexity)]
#[derive(Default, Debug)]
pub struct Handler {
    events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
            <Self as ConnectionHandler>::Error,
        >,
    >,

    entry: bool,
}

#[derive(Debug, Copy, Clone)]
pub enum In {
    Set,
    Unset,
}

impl ConnectionHandler for Handler {
    type FromBehaviour = In;
    type ToBehaviour = Void;
    type Error = Void;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = Void;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.entry {
            return KeepAlive::Yes;
        }
        KeepAlive::No
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            In::Set => self.entry = true,
            In::Unset => self.entry = false,
        }
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::RemoteProtocolsChange(_)
            | ConnectionEvent::FullyNegotiatedInbound(_)
            | ConnectionEvent::FullyNegotiatedOutbound(_)
            | ConnectionEvent::AddressChange(_)
            | ConnectionEvent::DialUpgradeError(_)
            | ConnectionEvent::ListenUpgradeError(_)
            | ConnectionEvent::LocalProtocolsChange(_) => {}
        }
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
            Self::Error,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }
        Poll::Pending
    }
}
