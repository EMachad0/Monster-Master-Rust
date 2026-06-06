use bevy::ecs::{event::Event, resource::Resource};

pub trait StdbConn: 'static + Send + Sync {}

impl<C: 'static + Send + Sync> StdbConn for C {}

/// The live SpacetimeDB connection. Generic over the concrete generated `DbConnection`.
#[derive(Clone, Resource)]
pub struct StdbConnection<C: StdbConn>(pub C);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Resource)]
pub enum StdbStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnected;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnected;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnectionError(ConnectionError);

impl StdbConnectionError {
    pub fn new(error: ConnectionError) -> Self {
        Self(error)
    }

    pub fn error(&self) -> ConnectionError {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionError {
    #[error("connection refused")]
    ConnectionRefused,
}
