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
        .add_observer(subscribe_to_players_on_connect)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn subscribe_to_players_on_connect(
    _: On<stdb_bevy::StdbConnected>,
    conn: Res<StdbConnection<DbConnection>>,
) {
    conn.subscription_builder()
        .on_applied(|_| info!("subscription applied"))
        .on_error(|_, err| error!("subscription error: {err}"))
        .subscribe(["SELECT * FROM player"]);
}

/// The connection proof: log the online player count whenever it changes.
fn report_players(conn: Res<StdbConnection<DbConnection>>, mut last: Local<i64>) {
    let online = conn.db().player().iter().filter(|p| p.online).count() as i64;
    if online != *last {
        *last = online;
        info!("{online} player(s) online");
    }
}
