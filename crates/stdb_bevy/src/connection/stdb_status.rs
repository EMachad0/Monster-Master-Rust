use bevy::ecs::{resource::Resource, system::Res};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Resource)]
pub enum StdbStatus {
    Connecting,
    Connected,
    Disconnected,
}

pub fn stdb_connected(status: Res<StdbStatus>) -> bool {
    *status == StdbStatus::Connected
}
