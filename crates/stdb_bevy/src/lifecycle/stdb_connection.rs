use bevy::ecs::{
    event::Event,
    observer::On,
    resource::Resource,
    system::{Res, ResMut},
};

pub trait StdbConn: 'static + Send + Sync {}

impl<C: 'static + Send + Sync> StdbConn for C {}

/// The live SpacetimeDB connection. Generic over the concrete generated `DbConnection`.
#[derive(Clone, Resource)]
pub struct StdbConnection<C: StdbConn>(pub C);

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

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnect;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) enum StdbIntent {
    Connected,
    Disconnected,
}

pub(crate) fn update_intent_on_stdbconnect(_: On<StdbConnect>, mut intent: ResMut<StdbIntent>) {
    *intent = StdbIntent::Connected;
}

pub(crate) fn update_intent_on_stdbdisconnect(
    _: On<StdbDisconnect>,
    mut intent: ResMut<StdbIntent>,
) {
    *intent = StdbIntent::Disconnected;
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Resource)]
pub enum StdbStatus {
    Connecting,
    Connected,
    Disconnected,
}

pub fn stdb_connected<C: StdbConn>(connection: Option<Res<StdbConnection<C>>>) -> bool {
    connection.is_some()
}
