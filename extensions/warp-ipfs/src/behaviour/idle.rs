mod handler;

use std::{
    task::{Context, Poll},
    time::Duration,
};

use rust_ipfs::libp2p::{
    core::Endpoint,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, PollParameters, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;

#[derive(Default)]
pub struct Behaviour;

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = handler::Handler;
    type ToSwarm = void::Void;

    fn handle_pending_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        Ok(())
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        Ok(vec![])
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(handler::Handler::new(Duration::from_secs(60)))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(handler::Handler::new(Duration::from_secs(60)))
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        _: THandlerOutEvent<Self>,
    ) {
    }

    fn on_swarm_event(&mut self, _: FromSwarm<Self::ConnectionHandler>) {}

    fn poll(
        &mut self,
        _: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}
