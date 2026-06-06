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
