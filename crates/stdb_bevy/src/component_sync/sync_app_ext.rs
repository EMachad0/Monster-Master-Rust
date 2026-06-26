use bevy::prelude::*;

use crate::{
    StdbSystemSet,
    component_sync::{
        projection::project_component_internal,
        row_entity_mapping::{
            SyncEntityMap, deregister_sync_entity_on_remove_stdbsync,
            register_sync_entity_on_add_stdbsync,
        },
        stdb_sync::{StdbSync, sync_component_internal},
    },
};

pub trait SyncAppExt {
    fn sync_component<S: StdbSync>(&mut self) -> &mut Self;

    fn projection<S, T>(&mut self) -> &mut Self
    where
        S: StdbSync,
        T: Component<Mutability = bevy::ecs::component::Mutable> + PartialEq + for<'s> From<&'s S>;
}

impl SyncAppExt for App {
    fn sync_component<S: StdbSync>(&mut self) -> &mut Self {
        self.init_resource::<SyncEntityMap<S>>()
            .add_observer(register_sync_entity_on_add_stdbsync::<S>)
            .add_observer(deregister_sync_entity_on_remove_stdbsync::<S>)
            .add_systems(
                Update,
                sync_component_internal::<S>.in_set(StdbSystemSet::Main),
            )
    }

    fn projection<S, T>(&mut self) -> &mut Self
    where
        S: StdbSync,
        T: Component<Mutability = bevy::ecs::component::Mutable> + PartialEq + for<'s> From<&'s S>,
    {
        self.add_systems(
            Update,
            project_component_internal::<S, T>
                .in_set(StdbSystemSet::Main)
                .after(sync_component_internal::<S>),
        )
    }
}
