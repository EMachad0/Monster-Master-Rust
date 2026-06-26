use bevy::prelude::*;
use spacetimedb_sdk::{DbContext, Identity};
use stdb_bevy::{RowDeleted, RowEntities, RowInserted, StdbConnection, StdbSync, Subscription};
use stdb_bindings::{DbConnection, set_cursor_position};

use crate::player::{Own, OwnedBy, Player};

pub fn subscribe_to_cursors(mut commands: Commands) {
    commands.spawn(Subscription::table("cursor"));
}

#[derive(Debug, Default, PartialEq, Clone, Copy, Component)]
pub struct Cursor {
    id: Identity,
    pub x: f32,
    pub y: f32,
}

impl From<&Cursor> for Transform {
    fn from(value: &Cursor) -> Self {
        Self::from_xyz(value.x, value.y, 0.0)
    }
}

impl From<&stdb_bindings::Cursor> for Cursor {
    fn from(value: &stdb_bindings::Cursor) -> Self {
        Self {
            id: value.id,
            x: value.x,
            y: value.y,
        }
    }
}

impl StdbSync for Cursor {
    type Row = stdb_bindings::Cursor;
    type Key = Identity;

    fn key(&self) -> Self::Key {
        self.id
    }
}

pub fn spawn_cursor_on_insert(
    mut reader: MessageReader<RowInserted<stdb_bindings::Cursor>>,
    mut commands: Commands,
    connection: Res<StdbConnection<DbConnection>>,
    player_entities: RowEntities<Player>,
    players: Query<&Player>,
) {
    for message in reader.read() {
        let player_identity = message.row().id;
        let player_entity = player_entities.single(&player_identity).unwrap();
        let player = players.get(player_entity).unwrap();

        let mut entity_commands = commands.spawn_scene(bsn! {
            Transform
            Mesh2d(asset_value(Circle::new(10.0)))
            MeshMaterial2d<ColorMaterial>(asset_value(player.color))
        });
        entity_commands.insert(OwnedBy(player_entity));
        entity_commands.insert(Cursor::from(message.row()));
        if connection.identity() == player_identity {
            entity_commands.insert(Own);
        }
    }
}

pub fn despawn_cursor_on_delete(
    mut reader: MessageReader<RowDeleted<stdb_bindings::Cursor>>,
    mut commands: Commands,
    row_entities: RowEntities<Cursor>,
) {
    for message in reader.read() {
        for entity in row_entities.get_by_row(message.row()) {
            commands.entity(*entity).despawn();
        }
    }
}

pub fn track_cursor(
    window: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    connection: Res<StdbConnection<DbConnection>>,
    cursor: Query<&Cursor, With<Own>>,
) {
    if let Ok(window) = window.single()
        && let Ok((camera, camera_transform)) = camera.single()
        && let Some(viewport_position) = window.cursor_position()
        && let Ok(Vec2 { x, y }) = camera.viewport_to_world_2d(camera_transform, viewport_position)
        && let Ok(Cursor { x: x0, y: y0, .. }) = cursor.single()
        && ((x - *x0).abs() > f32::EPSILON || (y - *y0).abs() > f32::EPSILON)
    {
        let _ = connection.reducers().set_cursor_position(x, y);
    }
}
