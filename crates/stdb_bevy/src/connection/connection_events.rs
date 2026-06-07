use bevy::ecs::event::Event;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnect;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnect;
