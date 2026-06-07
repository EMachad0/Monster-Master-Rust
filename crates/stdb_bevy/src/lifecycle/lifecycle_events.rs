use bevy::ecs::event::Event;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnected;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnected;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnectionError(ConnectionError);

impl StdbConnectionError {
    pub fn new(error: ConnectionError) -> Self {
        Self(error)
    }

    pub fn error(&self) -> ConnectionError {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionError {
    #[error("connection refused")]
    ConnectionRefused,
}
