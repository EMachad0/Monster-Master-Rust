use std::hash::Hash;

use bevy::{ecs::component::Mutable, prelude::*};

use crate::{RowUpdated, StdbRow, component_sync::row_entity_mapping::SyncEntityMap};

pub trait StdbSync:
    Component<Mutability = Mutable> + PartialEq + for<'r> From<&'r Self::Row>
{
    type Row: StdbRow;
    type Key: Hash + Eq + Send + Sync;

    fn key(&self) -> Self::Key;
}

pub(super) fn sync_component_internal<S: StdbSync>(
    mut updates: MessageReader<RowUpdated<S::Row>>,
    mut sync_components: Query<&mut S>,
    sync_entity_map: Res<SyncEntityMap<S>>,
) {
    for RowUpdated { new, .. } in updates.read() {
        if let Some(entities) = sync_entity_map.get(&S::from(new).key()) {
            for entity in entities.iter() {
                match sync_components.get_mut(*entity) {
                    Ok(mut old_comp) => {
                        old_comp.set_if_neq(S::from(new));
                    }
                    Err(_e) => {
                        bevy::log::warn!(
                            "sync map referenced an entity despawned or missing its component"
                        )
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::RowUpdated;
    use crate::component_sync::row_entity_mapping::{
        deregister_sync_entity_on_remove_stdbsync, register_sync_entity_on_add_stdbsync,
    };

    /// A stand-in server row. A real Module row is `Clone + Send + Sync + 'static + PartialEq`.
    #[derive(Clone, PartialEq, Debug)]
    struct HealthRow {
        id: u32,
        hp: u32,
    }

    /// The self-keying component: it carries its own key (`entity_id`), so the sync scan matches a
    /// row to its entity with no separate tag.
    #[derive(Component, PartialEq, Debug)]
    struct Health {
        entity_id: u32,
        hp: u32,
    }

    /// Built from a borrow of the row (no clone): the one conversion the Game's spawn and the
    /// Bridge's update both reuse.
    impl From<&HealthRow> for Health {
        fn from(row: &HealthRow) -> Self {
            Health {
                entity_id: row.id,
                hp: row.hp,
            }
        }
    }

    impl StdbSync for Health {
        type Row = HealthRow;
        type Key = u32;

        fn key(&self) -> u32 {
            self.entity_id
        }
    }

    /// An app with the index machinery wired the way `SyncAppExt::sync_component` will wire it: the
    /// key-to-entities map, the add and remove observers that maintain it, and the sync system under
    /// test. Messages are fed directly, with no channel or connection.
    fn health_app() -> App {
        let mut app = App::new();
        app.add_message::<RowUpdated<HealthRow>>();
        app.init_resource::<SyncEntityMap<Health>>();
        app.add_observer(register_sync_entity_on_add_stdbsync::<Health>);
        app.add_observer(deregister_sync_entity_on_remove_stdbsync::<Health>);
        app.add_systems(Update, sync_component_internal::<Health>);
        app
    }

    /// Records which entities a frame saw as `Changed<Health>`, so a test can assert change
    /// detection. Ordered after the sync system in `health_app_with_change_log`.
    #[derive(Resource, Default)]
    struct ChangedLog(Vec<u32>);

    fn record_changed(query: Query<&Health, Changed<Health>>, mut log: ResMut<ChangedLog>) {
        for health in &query {
            log.0.push(health.entity_id);
        }
    }

    /// Like `health_app`, plus a recorder ordered after the sync so a test can observe whether an
    /// update fired `Changed<Health>`.
    fn health_app_with_change_log() -> App {
        let mut app = health_app();
        app.init_resource::<ChangedLog>();
        app.add_systems(
            Update,
            record_changed.after(sync_component_internal::<Health>),
        );
        app
    }

    #[test]
    fn update_to_a_matching_key_writes_the_new_value() {
        let mut app = health_app();
        let entity = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();

        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 1, hp: 100 },
            HealthRow { id: 1, hp: 80 },
        ));
        app.update();

        assert_eq!(
            app.world().entity(entity).get::<Health>().unwrap().hp,
            80,
            "an update whose key matches the entity must write the new row's value",
        );
    }

    #[test]
    fn a_real_value_change_marks_the_component_changed() {
        let mut app = health_app_with_change_log();
        app.world_mut().spawn(Health {
            entity_id: 1,
            hp: 100,
        });

        // Settle one frame so the spawn's own change flag ages out, then clear the log: only a
        // change caused by the next update should be observed.
        app.update();
        app.world_mut().resource_mut::<ChangedLog>().0.clear();

        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 1, hp: 100 },
            HealthRow { id: 1, hp: 80 },
        ));
        app.update();

        assert_eq!(
            app.world().resource::<ChangedLog>().0,
            vec![1],
            "a genuine value change must fire Changed<Health> so change-driven systems can react",
        );
    }

    #[test]
    fn one_update_reaches_every_entity_sharing_the_key() {
        let mut app = health_app();
        let a = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();
        let b = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();

        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 1, hp: 100 },
            HealthRow { id: 1, hp: 80 },
        ));
        app.update();

        assert_eq!(app.world().entity(a).get::<Health>().unwrap().hp, 80);
        assert_eq!(
            app.world().entity(b).get::<Health>().unwrap().hp,
            80,
            "one row may back several entities, so every entity carrying the key is updated",
        );
    }

    #[test]
    fn update_to_a_non_matching_key_leaves_entities_untouched() {
        let mut app = health_app();
        let entity = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();

        // The update targets key 2; the only entity carries key 1.
        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 2, hp: 100 },
            HealthRow { id: 2, hp: 80 },
        ));
        app.update();

        assert_eq!(
            app.world().entity(entity).get::<Health>().unwrap().hp,
            100,
            "an update whose key matches no entity must leave every entity untouched",
        );
    }

    #[test]
    fn a_no_op_update_does_not_mark_the_component_changed() {
        let mut app = health_app_with_change_log();
        app.world_mut().spawn(Health {
            entity_id: 1,
            hp: 100,
        });

        app.update();
        app.world_mut().resource_mut::<ChangedLog>().0.clear();

        // `new` rebuilds the same value the component already holds.
        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 1, hp: 100 },
            HealthRow { id: 1, hp: 100 },
        ));
        app.update();

        assert!(
            app.world().resource::<ChangedLog>().0.is_empty(),
            "set-if-changed: an update equal to the current value must not fire Changed<Health>",
        );
    }

    #[test]
    fn an_update_with_no_matching_entity_is_a_safe_no_op() {
        let mut app = health_app();
        // No entity carries Health at all.

        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 1, hp: 100 },
            HealthRow { id: 1, hp: 80 },
        ));
        app.update(); // must not panic

        let mut query = app.world_mut().query::<&Health>();
        assert_eq!(
            query.iter(app.world()).count(),
            0,
            "an update with no entity to match does nothing: it spawns nothing and does not panic",
        );
    }

    #[test]
    fn despawning_one_of_several_entities_sharing_a_key_still_updates_the_rest() {
        let mut app = health_app();
        let a = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();
        let b = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();

        // Despawn one entity; the other must keep receiving updates for the shared key.
        app.world_mut().entity_mut(a).despawn();

        app.world_mut().write_message(RowUpdated::new(
            HealthRow { id: 1, hp: 100 },
            HealthRow { id: 1, hp: 80 },
        ));
        app.update();

        assert_eq!(
            app.world().entity(b).get::<Health>().unwrap().hp,
            80,
            "despawning one entity for a key must not stop the remaining entities from updating",
        );
    }

    #[test]
    fn despawning_an_entity_prunes_it_from_the_sync_map() {
        let mut app = health_app();
        let entity = app
            .world_mut()
            .spawn(Health {
                entity_id: 1,
                hp: 100,
            })
            .id();
        app.world_mut().entity_mut(entity).despawn();

        // Cleanup has no behavioral signal (a stale entry is silently skipped on update), so this
        // asserts the internal map directly to guard against an unbounded leak under entity churn.
        let map = app.world().resource::<SyncEntityMap<Health>>();
        assert!(
            map.get(&1u32).is_none_or(|entities| entities.is_empty()),
            "despawn must prune the entity from its key bucket so the map does not leak dead entities",
        );
    }
}
