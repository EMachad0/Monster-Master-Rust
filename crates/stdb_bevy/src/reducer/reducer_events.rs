use std::marker::PhantomData;

use bevy::ecs::event::Event;

/// Fired when a Reducer this client invoked committed. `K` is the Game's per-reducer marker, so a
/// system reacts to one reducer's outcome with `On<ReducerCommitted<MyReducer>>`.
#[derive(Event)]
pub struct ReducerCommitted<K> {
    _marker: PhantomData<K>,
}

impl<K> ReducerCommitted<K> {
    pub(crate) fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

/// Fired when a Reducer this client invoked failed. Covers both a returned error and a host abort;
/// both carry a human-readable message.
#[derive(Event)]
pub struct ReducerFailed<K> {
    error: String,
    _marker: PhantomData<K>,
}

impl<K> ReducerFailed<K> {
    pub(crate) fn new(error: String) -> Self {
        Self {
            error,
            _marker: PhantomData,
        }
    }

    pub fn error(&self) -> &str {
        &self.error
    }
}
