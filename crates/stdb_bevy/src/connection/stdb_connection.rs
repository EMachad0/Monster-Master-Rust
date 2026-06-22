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

#[derive(Resource)]
pub struct StdbPreviousConnection<C: StdbConn>(pub C);

impl<C: StdbConn> std::ops::Deref for StdbPreviousConnection<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) fn resync_messages_on_reconnect<C: StdbConn>(mut commands: Commands) {
    commands.remove_resource::<StdbPreviousConnection<C>>();
}
