use bevy::ecs::resource::Resource;

pub trait StdbConn: 'static + Send + Sync {}

/// The live SpacetimeDB connection. Generic over the concrete generated `DbConnection`.
#[derive(Clone, Resource)]
pub struct StdbConnection<C: StdbConn>(pub C);
