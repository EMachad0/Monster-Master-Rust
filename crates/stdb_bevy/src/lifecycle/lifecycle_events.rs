use bevy::ecs::event::Event;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbConnected;

#[derive(Debug, Copy, Clone, Event)]
pub struct StdbDisconnected;

#[derive(Debug, Clone, Event)]
pub struct StdbConnectionError(ConnectionError);

impl StdbConnectionError {
    pub fn new(error: ConnectionError) -> Self {
        Self(error)
    }

    pub fn error(&self) -> &ConnectionError {
        &self.0
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectionError {
    #[error("Connection Refused")]
    ConnectionRefused,

    #[error(transparent)]
    SdkError(#[from] spacetimedb_sdk::Error),
}
