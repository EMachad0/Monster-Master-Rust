use std::any::type_name;

use bevy::{ecs::component::Mutable, prelude::*};

pub(super) fn project_component_internal<S, T>(mut query: Query<(&S, Option<&mut T>), Changed<S>>)
where
    S: Component,
    T: Component<Mutability = Mutable> + PartialEq + for<'s> From<&'s S>,
{
    for (sync_component, component) in query.iter_mut() {
        if let Some(mut component) = component {
            component.set_if_neq(sync_component.into());
        } else {
            bevy::log::warn!(
                "attempting to project {} into {} but the latter is not present",
                type_name::<S>(),
                type_name::<T>(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::StdbSync;

    /// A stand-in server row for the synced source component.
    #[derive(Clone, PartialEq, Debug)]
    struct PositionRow {
        id: u32,
        x: f32,
        y: f32,
        z: f32,
    }

    /// The source. The engine needs only `S: Component`; here it is also a `StdbSync` component, the
    /// first hop of the two-hop derive, mirroring real usage (a synced component projected onward).
    #[derive(Component, Clone, Copy, PartialEq, Debug)]
    struct Position {
        entity_id: u32,
        x: f32,
        y: f32,
        z: f32,
    }

    impl From<&PositionRow> for Position {
        fn from(row: &PositionRow) -> Self {
            Position {
                entity_id: row.id,
                x: row.x,
                y: row.y,
                z: row.z,
            }
        }
    }

    impl StdbSync for Position {
        type Row = PositionRow;
        type Key = u32;

        fn key(&self) -> u32 {
            self.entity_id
        }
    }

    /// The projection: a local source into a foreign target. The orphan rule allows it because
    /// `Position` is local. It deliberately drops `z` (render z is fixed), which
    /// `a_source_change_that_projects_equal_does_not_refire_changed` relies on.
    impl From<&Position> for Transform {
        fn from(p: &Position) -> Self {
            Transform::from_xyz(p.x, p.y, 0.0)
        }
    }

    /// An app with only the projection system installed. `Position` is set and changed directly: the
    /// projection is the second hop, downstream of the synced component, so no row sync is involved.
    fn position_app() -> App {
        let mut app = App::new();
        app.add_systems(Update, project_component_internal::<Position, Transform>);
        app
    }

    /// Records which entities a frame saw as `Changed<Transform>`. Ordered after the projection in
    /// `position_app_with_change_log`.
    #[derive(Resource, Default)]
    struct ChangedLog(Vec<Entity>);

    fn record_changed(query: Query<Entity, Changed<Transform>>, mut log: ResMut<ChangedLog>) {
        for entity in &query {
            log.0.push(entity);
        }
    }

    /// Like `position_app`, plus a recorder ordered after the projection so a test can observe
    /// whether it fired `Changed<Transform>`.
    fn position_app_with_change_log() -> App {
        let mut app = position_app();
        app.init_resource::<ChangedLog>();
        app.add_systems(
            Update,
            record_changed.after(project_component_internal::<Position, Transform>),
        );
        app
    }

    #[test]
    fn the_projection_does_not_create_an_absent_target() {
        let mut app = position_app();
        let entity = app
            .world_mut()
            .spawn(Position {
                entity_id: 1,
                x: 1.0,
                y: 2.0,
                z: 9.0,
            })
            .id();

        app.update();

        assert!(
            app.world().entity(entity).get::<Transform>().is_none(),
            "the projection only syncs an existing target; with none present it must not create one \
             (the Game composes the entity with its target component)",
        );
    }

    #[test]
    fn a_changed_source_corrects_an_existing_stale_target() {
        let mut app = position_app();
        let entity = app
            .world_mut()
            .spawn((
                Position {
                    entity_id: 1,
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                },
                Transform::from_xyz(99.0, 99.0, 99.0),
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().entity(entity).get::<Transform>().copied(),
            Some(Transform::from_xyz(1.0, 2.0, 0.0)),
            "a changed source must overwrite an existing target with the projected value",
        );
    }

    #[test]
    fn a_real_projected_change_marks_the_target_changed() {
        let mut app = position_app_with_change_log();
        let entity = app
            .world_mut()
            .spawn((
                Position {
                    entity_id: 1,
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                },
                Transform::from_xyz(1.0, 2.0, 0.0),
            ))
            .id();

        // Settle one frame so the spawn's own change flag ages out, then clear the log: only a
        // change caused by the next source edit should be observed.
        app.update();
        app.world_mut().resource_mut::<ChangedLog>().0.clear();

        // A real change to the projected value (x moves).
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Position>()
            .unwrap()
            .x = 5.0;
        app.update();

        assert_eq!(
            app.world().resource::<ChangedLog>().0,
            vec![entity],
            "a source change that alters the projection must fire Changed<Transform>",
        );
    }

    #[test]
    fn an_unchanged_source_does_not_clobber_an_externally_modified_target() {
        let mut app = position_app();
        let entity = app
            .world_mut()
            .spawn((
                Position {
                    entity_id: 1,
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                },
                Transform::from_xyz(1.0, 2.0, 0.0),
            ))
            .id();

        // Settle so the source is no longer Changed.
        app.update();

        // Another writer nudges the target; the source has not changed.
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Transform>()
            .unwrap()
            .translation
            .x = 42.0;
        app.update();

        assert_eq!(
            app.world()
                .entity(entity)
                .get::<Transform>()
                .unwrap()
                .translation
                .x,
            42.0,
            "with the source unchanged, the Changed<R> gate must leave an externally edited target \
             alone rather than reverting it every frame",
        );
    }

    #[test]
    fn a_source_change_that_projects_equal_does_not_refire_changed() {
        let mut app = position_app_with_change_log();
        let entity = app
            .world_mut()
            .spawn((
                Position {
                    entity_id: 1,
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                },
                Transform::from_xyz(1.0, 2.0, 0.0),
            ))
            .id();

        app.update();
        app.world_mut().resource_mut::<ChangedLog>().0.clear();

        // Change only z, which the projection drops, so the projected Transform is identical.
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Position>()
            .unwrap()
            .z = 5.0;
        app.update();

        assert!(
            app.world().resource::<ChangedLog>().0.is_empty(),
            "a source change that projects to the same value must not re-fire Changed<Transform>",
        );
    }
}
