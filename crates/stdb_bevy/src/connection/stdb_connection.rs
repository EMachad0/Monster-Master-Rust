use bevy::ecs::resource::Resource;

pub trait StdbConn: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> StdbConn for T {}

/// The live SpacetimeDB connection. Generic over the concrete generated `DbConnection`.
#[derive(Clone, Resource)]
pub struct StdbConnection<C: StdbConn>(pub C);

impl<C: StdbConn> std::ops::Deref for StdbConnection<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
