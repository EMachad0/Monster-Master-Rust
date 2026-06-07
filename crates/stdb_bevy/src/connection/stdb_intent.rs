use bevy::ecs::{observer::On, resource::Resource, system::ResMut};

use crate::connection::connection_events::{StdbConnect, StdbDisconnect};

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
