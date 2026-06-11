use bevy::ecs::{event::Event, system::Commands};

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnect;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnect;

pub(crate) fn trigger_connect(mut commands: Commands) {
    commands.trigger(StdbConnect);
}
