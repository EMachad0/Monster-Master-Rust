//! End-to-end coverage of the Row mirror's public surface (`SyncAppExt`): that
//! `sync_component` and `projection` wire their machinery and run in the right order, driven only
//! through the crate's public API. The row pipeline (callback -> channel -> `RowUpdated`) is covered
//! by the bridge's own tests, so these inject `RowUpdated` directly to isolate the wiring.

use bevy::prelude::*;
use stdb_bevy::{RowUpdated, StdbSync, SyncAppExt};

/// A stand-in server row. `add_message::<RowUpdated<PlayerRow>>()` stands in for the message that
/// table registration installs in production.
#[derive(Clone, PartialEq, Debug)]
struct PlayerRow {
    id: u32,
    x: f32,
    y: f32,
}

/// A self-keying synced component (the first hop).
#[derive(Component, Clone, Copy, PartialEq, Debug)]
struct Position {
    entity_id: u32,
    x: f32,
    y: f32,
}

impl From<&PlayerRow> for Position {
    fn from(row: &PlayerRow) -> Self {
        Position {
            entity_id: row.id,
            x: row.x,
            y: row.y,
        }
    }
}

impl StdbSync for Position {
    type Row = PlayerRow;
    type Key = u32;

    fn key(&self) -> u32 {
        self.entity_id
    }
}

/// The second hop: a local source projected into the foreign `Transform`.
impl From<&Position> for Transform {
    fn from(p: &Position) -> Self {
        Transform::from_xyz(p.x, p.y, 0.0)
    }
}

fn app_with_sync() -> App {
    let mut app = App::new();
    app.add_message::<RowUpdated<PlayerRow>>();
    app.sync_component::<Position>();
    app
}

fn app_with_sync_and_projection() -> App {
    let mut app = app_with_sync();
    app.projection::<Position, Transform>();
    app
}

#[test]
fn sync_component_extension_applies_a_row_update_to_the_component() {
    let mut app = app_with_sync();
    let entity = app
        .world_mut()
        .spawn(Position {
            entity_id: 1,
            x: 0.0,
            y: 0.0,
        })
        .id();

    app.world_mut().write_message(RowUpdated::new(
        PlayerRow {
            id: 1,
            x: 0.0,
            y: 0.0,
        },
        PlayerRow {
            id: 1,
            x: 5.0,
            y: 6.0,
        },
    ));
    app.update();

    assert_eq!(
        app.world().entity(entity).get::<Position>().copied(),
        Some(Position {
            entity_id: 1,
            x: 5.0,
            y: 6.0,
        }),
        "one sync_component call must wire the map, observers, and system so a row update reaches \
         the component",
    );
}

#[test]
fn a_row_update_reaches_both_the_synced_component_and_the_projection_in_one_frame() {
    let mut app = app_with_sync_and_projection();
    let entity = app
        .world_mut()
        .spawn((
            Position {
                entity_id: 1,
                x: 0.0,
                y: 0.0,
            },
            Transform::default(),
        ))
        .id();

    // Settle so the change observed below comes from the row update, not the spawn.
    app.update();

    app.world_mut().write_message(RowUpdated::new(
        PlayerRow {
            id: 1,
            x: 0.0,
            y: 0.0,
        },
        PlayerRow {
            id: 1,
            x: 5.0,
            y: 6.0,
        },
    ));
    app.update();

    assert_eq!(
        app.world().entity(entity).get::<Position>().copied(),
        Some(Position {
            entity_id: 1,
            x: 5.0,
            y: 6.0,
        }),
        "the row update reaches the synced component",
    );
    assert_eq!(
        app.world().entity(entity).get::<Transform>().copied(),
        Some(Transform::from_xyz(5.0, 6.0, 0.0)),
        "and the projection lands in the same frame, proving it runs after the sync (otherwise the \
         target would lag a frame behind the source)",
    );
}

#[test]
fn projection_registered_without_its_sync_does_not_panic() {
    let mut app = App::new();
    // No sync_component, so the projection's `.after(sync_component_internal::<Position>)` orders
    // against a system that is not in the schedule.
    app.projection::<Position, Transform>();
    let entity = app
        .world_mut()
        .spawn((
            Position {
                entity_id: 1,
                x: 1.0,
                y: 2.0,
            },
            Transform::default(),
        ))
        .id();

    app.update();

    assert_eq!(
        app.world().entity(entity).get::<Transform>().copied(),
        Some(Transform::from_xyz(1.0, 2.0, 0.0)),
        "projection registered without its sync must still run rather than crash at schedule build",
    );
}
