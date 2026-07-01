use bevy::{
    ecs::{query::QuerySingleError, system::SystemParam},
    platform::collections::HashMap,
    prelude::*,
};
use smallvec::SmallVec;

use crate::StdbSync;

/// Read-only lookup from a [`StdbSync::Key`] (or a row) to the entities currently mirroring it.
///
/// Spares the Game its own row-to-entity map: the Bridge already keeps this index to drive the mirror.
/// Covers only tables the Game mirrors.
#[derive(SystemParam)]
pub struct RowEntities<'w, S>
where
    S: StdbSync,
{
    entity_map: Res<'w, SyncEntityMap<S>>,
}

impl<'w, S: StdbSync> RowEntities<'w, S> {
    /// Every entity carrying `key`; empty when none do.
    pub fn get(&self, key: &S::Key) -> &[Entity] {
        self.entity_map
            .get(key)
            .map(SmallVec::as_slice)
            .unwrap_or_default()
    }

    /// The sole entity for `key`, or a `QuerySingleError` when none or several carry it (the 1:1
    /// join).
    pub fn single(&self, key: &S::Key) -> Result<Entity, QuerySingleError> {
        let v = self.get(key);
        match v.len() {
            0 => Err(QuerySingleError::NoEntities(DebugName::type_name::<S>())),
            1 => Ok(v[0]),
            _ => Err(QuerySingleError::MultipleEntities(
                DebugName::type_name::<S>(),
            )),
        }
    }

    /// Like [`get`](Self::get), deriving the key from `row`.
    pub fn get_by_row(&self, row: &S::Row) -> &[Entity] {
        let key = S::from(row).key();
        self.get(&key)
    }

    /// Like [`single`](Self::single), deriving the key from `row`.
    pub fn single_by_row(&self, row: &S::Row) -> Result<Entity, QuerySingleError> {
        let key = S::from(row).key();
        self.single(&key)
    }
}

/// Index from a mirror key to the entities carrying that component, maintained by the add/remove
/// observers below. The per-key `SmallVec` lets one row back several entities.
#[derive(Resource, Deref, DerefMut)]
pub(super) struct SyncEntityMap<R: StdbSync>(HashMap<R::Key, SmallVec<[Entity; 4]>>);

impl<R: StdbSync> SyncEntityMap<R> {
    pub fn new() -> Self {
        Self(HashMap::default())
    }
}

impl<R: StdbSync> Default for SyncEntityMap<R> {
    fn default() -> Self {
        Self::new()
    }
}

/// Files a newly added mirror component under its key. With the removal observer this is the only
/// place the index is written; the update path never re-files, so the key must be immutable.
pub(super) fn register_sync_entity_on_add_stdbsync<S: StdbSync>(
    observer: On<Add, S>,
    mut sync_entity_map: ResMut<SyncEntityMap<S>>,
    sync_components: Query<&S>,
) {
    let entity = observer.entity;
    match sync_components.get(entity) {
        Ok(c) => sync_entity_map.entry(c.key()).or_default().push(entity),
        Err(e) => bevy::log::error!("{}", e),
    };
}

/// Removes a despawned or dropped mirror component from its key bucket, so the index does not leak
/// dead entities.
pub(super) fn deregister_sync_entity_on_remove_stdbsync<S: StdbSync>(
    observer: On<Remove, S>,
    mut sync_entity_map: ResMut<SyncEntityMap<S>>,
    sync_components: Query<&S>,
) {
    let entity = observer.entity;
    match sync_components.get(entity) {
        Ok(c) => {
            if let Some(v) = sync_entity_map.get_mut(&c.key()) {
                if let Some(idx) = v.iter().position(|e| *e == entity) {
                    v.swap_remove(idx);
                } else {
                    bevy::log::warn!(
                        "attempting to remove entity from SyncEntityMap but it is already removed"
                    );
                }
            } else {
                bevy::log::warn!(
                    "attempting to remove entity from initialized key in SyncEntityMap"
                );
            }
        }
        Err(e) => bevy::log::error!("{}", e),
    };
}

#[cfg(test)]
mod tests {
    use bevy::ecs::query::QuerySingleError;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use super::*;

    /// A stand-in server row. A real Module row is `Clone + Send + Sync + 'static + PartialEq`.
    #[derive(Clone, PartialEq, Debug)]
    struct WidgetRow {
        id: u32,
    }

    /// The self-keying mirror component: it carries its own key (`id`), so the index buckets it by
    /// that key with no separate tag.
    #[derive(Component, PartialEq, Debug)]
    struct Widget {
        id: u32,
    }

    impl From<&WidgetRow> for Widget {
        fn from(row: &WidgetRow) -> Self {
            Widget { id: row.id }
        }
    }

    impl StdbSync for Widget {
        type Row = WidgetRow;
        type Key = u32;

        fn key(&self) -> u32 {
            self.id
        }
    }

    /// An app with the index machinery wired the way `sync_component` wires it: the key-to-entities
    /// map plus the add/remove observers that maintain it. No connection or messages — entities are
    /// spawned directly and read back through `RowEntities`.
    fn index_app() -> App {
        let mut app = App::new();
        app.init_resource::<SyncEntityMap<Widget>>();
        app.add_observer(register_sync_entity_on_add_stdbsync::<Widget>);
        app.add_observer(deregister_sync_entity_on_remove_stdbsync::<Widget>);
        app
    }

    #[test]
    fn get_returns_every_entity_carrying_the_key() {
        let mut app = index_app();
        let a = app.world_mut().spawn(Widget { id: 1 }).id();
        let b = app.world_mut().spawn(Widget { id: 1 }).id();

        let found = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.get(&1).to_vec())
            .unwrap();

        assert_eq!(
            found.len(),
            2,
            "one row may back several entities, so get returns every entity carrying the key",
        );
        assert!(
            found.contains(&a) && found.contains(&b),
            "get returns exactly the entities carrying the queried key",
        );
    }

    #[test]
    fn get_excludes_entities_carrying_a_different_key() {
        let mut app = index_app();
        let one = app.world_mut().spawn(Widget { id: 1 }).id();
        let _two = app.world_mut().spawn(Widget { id: 2 }).id();

        let found = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.get(&1).to_vec())
            .unwrap();

        assert_eq!(
            found,
            vec![one],
            "get(&1) returns only the entity under key 1, never entities carrying other keys",
        );
    }

    #[test]
    fn get_returns_an_empty_slice_for_an_absent_key() {
        let mut app = index_app();
        // A populated key exists, but it is not the one we query.
        app.world_mut().spawn(Widget { id: 1 });

        let found = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.get(&99).to_vec())
            .unwrap();

        assert!(
            found.is_empty(),
            "an absent key must yield an empty slice, never a panic — despawn and join sites \
             call get on keys that may not be present",
        );
    }

    #[test]
    fn single_returns_the_sole_entity_for_a_key() {
        let mut app = index_app();
        let only = app.world_mut().spawn(Widget { id: 1 }).id();

        let result = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.single(&1))
            .unwrap();

        assert_eq!(
            result.ok(),
            Some(only),
            "exactly one entity under the key resolves to that entity (the 1:1 join)",
        );
    }

    #[test]
    fn single_errs_no_entities_for_an_absent_key() {
        let mut app = index_app();
        // A populated key exists, but it is not the one we query.
        app.world_mut().spawn(Widget { id: 1 });

        let result = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.single(&99))
            .unwrap();

        assert!(
            matches!(result, Err(QuerySingleError::NoEntities(_))),
            "a key with no entity resolves to NoEntities, the join's unknown-key signal",
        );
    }

    #[test]
    fn single_errs_multiple_entities_when_several_share_a_key() {
        let mut app = index_app();
        app.world_mut().spawn(Widget { id: 1 });
        app.world_mut().spawn(Widget { id: 1 });

        let result = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.single(&1))
            .unwrap();

        assert!(
            matches!(result, Err(QuerySingleError::MultipleEntities(_))),
            "several entities sharing a key resolves to MultipleEntities, never a silent first-of-many",
        );
    }

    #[test]
    fn get_by_row_returns_the_entities_under_the_rows_key() {
        let mut app = index_app();
        let a = app.world_mut().spawn(Widget { id: 1 }).id();
        let b = app.world_mut().spawn(Widget { id: 1 }).id();

        let found = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| {
                index.get_by_row(&WidgetRow { id: 1 }).to_vec()
            })
            .unwrap();

        assert_eq!(
            found.len(),
            2,
            "get_by_row derives the key from the row and returns every entity under it",
        );
        assert!(
            found.contains(&a) && found.contains(&b),
            "the row's id is projected to the same key the entities were indexed under",
        );
    }

    #[test]
    fn single_by_row_resolves_the_sole_entity_for_the_rows_key() {
        let mut app = index_app();
        let only = app.world_mut().spawn(Widget { id: 1 }).id();

        let result = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| index.single_by_row(&WidgetRow { id: 1 }))
            .unwrap();

        assert_eq!(
            result.ok(),
            Some(only),
            "single_by_row derives the key from the row and resolves its sole entity",
        );
    }

    #[test]
    fn get_by_row_returns_an_empty_slice_for_an_unmatched_row() {
        let mut app = index_app();
        // An entity exists under key 1, but the row we query carries a different key.
        app.world_mut().spawn(Widget { id: 1 });

        let found = app
            .world_mut()
            .run_system_once(|index: RowEntities<Widget>| {
                index.get_by_row(&WidgetRow { id: 99 }).to_vec()
            })
            .unwrap();

        assert!(
            found.is_empty(),
            "a row whose derived key matches no entity yields an empty slice, never a panic",
        );
    }
}
