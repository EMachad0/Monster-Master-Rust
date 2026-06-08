use bevy::prelude::*;
use spacetimedb_sdk::{DbContext, Table};
use stdb_bevy::{SdkConnectionDriver, StdbConnection, StdbPlugin, is_stdb_connected};
use stdb_bindings::{DbConnection, PlayerTableAccess};

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
            StdbPlugin::new(SdkConnectionDriver {
                uri: "http://127.0.0.1:3000".try_into().unwrap(),
                database_name: "monster-master".to_string(),
                tick: DbConnection::frame_tick,
            })
            .with_connect_on_startup(),
        )
        .add_systems(Startup, setup)
        .add_systems(Update, report_players.run_if(is_stdb_connected))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// The connection proof: log the online player count whenever it changes.
fn report_players(conn: Res<StdbConnection<DbConnection>>, mut last: Local<i64>) {
    let online = conn.0.db().player().iter().filter(|p| p.online).count() as i64;
    if online != *last {
        *last = online;
        info!("{online} player(s) online");
    }
}
