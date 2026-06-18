use bevy::prelude::*;

pub trait StdbConn: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> StdbConn for T {}

/// The live SpacetimeDB connection. Generic over the concrete generated `DbConnection`.
#[derive(Resource)]
pub struct StdbConnection<C: StdbConn>(pub C);

impl<C: StdbConn> std::ops::Deref for StdbConnection<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Resource)]
pub struct StdbPreviousConnection<C: StdbConn>(pub C);

impl<C: StdbConn> std::ops::Deref for StdbPreviousConnection<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<C: StdbConn> From<StdbConnection<C>> for StdbPreviousConnection<C> {
    fn from(value: StdbConnection<C>) -> Self {
        Self(value.0)
    }
}
