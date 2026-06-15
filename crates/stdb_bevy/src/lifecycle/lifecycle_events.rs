use bevy::ecs::event::Event;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnected;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnected;
