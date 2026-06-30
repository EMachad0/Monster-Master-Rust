use bevy::prelude::*;
use stdb_bevy::{StdbPlugin, SyncAppExt, is_stdb_connected, stdb_table};
use stdb_bindings::{CursorTableAccess, DbConnection, PlayerTableAccess};

mod cursor;
mod player;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Monster Master".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(
            StdbPlugin::sdk(
                "http://127.0.0.1:3000",
                "monster-master",
                DbConnection::frame_tick,
            )
            .add_tables([
                stdb_table!(player => Player, key = identity),
                stdb_table!(cursor => Cursor, key = id),
            ])
            // .add_tables([stdb_bevy::TableRegistration::pk(
            //     |conn, fwd| fwd.forward(&conn.db().player()),
            //     |conn| conn.db().player().iter().collect(),
            //     |row| row.identity,
            //     stdb_bevy::RowMessagesMask::ALL,
            //     "player",
            // )])
            .with_connect_on_startup(),
        )
        .init_resource::<player::OnlineCounter>()
        .sync_component::<player::Player>()
        .sync_component::<cursor::Cursor>()
        .projection::<cursor::Cursor, Transform>()
        .add_systems(
            Startup,
            (
                setup,
                player::subscribe_to_players,
                cursor::subscribe_to_cursors,
            ),
        )
        .add_systems(
            Update,
            (
                (
                    player::spawn_player_on_insert,
                    player::despawn_player_on_delete,
                ),
                (
                    cursor::spawn_cursor_on_insert,
                    cursor::despawn_cursor_on_delete,
                ),
                cursor::track_cursor,
            )
                .chain()
                .run_if(is_stdb_connected),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}
