use bevy::prelude::*;

use crate::{
    StdbSystemSet,
    component_sync::stdb_sync::{
        StdbSync, SyncEntityMap, deregister_sync_entity_on_remove_stdbsync,
        register_sync_entity_on_add_stdbsync, sync_component_internal,
    },
};

pub trait SyncAppExt {
    fn sync_component<S: StdbSync>(&mut self);
}

impl SyncAppExt for App {
    fn sync_component<S: StdbSync>(&mut self) {
        self.init_resource::<SyncEntityMap<S>>();

        self.add_observer(register_sync_entity_on_add_stdbsync::<S>);
        self.add_observer(deregister_sync_entity_on_remove_stdbsync::<S>);
        self.add_systems(
            Update,
            sync_component_internal::<S>.in_set(StdbSystemSet::Main),
        );
    }
}
