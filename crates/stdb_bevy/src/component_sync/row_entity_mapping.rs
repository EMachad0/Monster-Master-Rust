use bevy::{
    ecs::{query::QuerySingleError, system::SystemParam},
    platform::collections::HashMap,
    prelude::*,
};
use smallvec::SmallVec;

use crate::StdbSync;

#[derive(SystemParam)]
pub struct RowEntities<'w, S>
where
    S: StdbSync,
{
    entity_map: Res<'w, SyncEntityMap<S>>,
}

impl<'w, S: StdbSync> RowEntities<'w, S> {
    pub fn get(&self, key: &S::Key) -> &[Entity] {
        self.entity_map
            .get(key)
            .map(SmallVec::as_slice)
            .unwrap_or_default()
    }

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

    pub fn get_by_row(&self, row: &S::Row) -> &[Entity] {
        let key = S::from(row).key();
        self.get(&key)
    }

    pub fn single_by_row(&self, row: &S::Row) -> Result<Entity, QuerySingleError> {
        let key = S::from(row).key();
        self.single(&key)
    }
}

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
}
