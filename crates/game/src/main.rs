use bevy::prelude::*;
use stdb_bevy::{RowDeleted, RowInserted, StdbPlugin, Subscription, is_stdb_connected, stdb_table};
use stdb_bindings::{DbConnection, Player, PlayerTableAccess};

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
            .add_tables([stdb_table!(player => Player, key = identity)])
            // .add_tables([stdb_bevy::TableRegistration::pk(
            //     |conn, fwd| fwd.forward(&conn.db().player()),
            //     |conn| conn.db().player().iter().collect(),
            //     |row| row.identity,
            // )])
            .with_connect_on_startup(),
        )
        .init_resource::<OnlineCounter>()
        .add_systems(Startup, (setup, subscribe_to_players))
        .add_systems(
            Update,
            (
                report_players_on_player_inserted,
                report_players_on_player_deleted,
            )
                .run_if(is_stdb_connected),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn subscribe_to_players(mut commands: Commands) {
    commands.spawn(Subscription::query(
        "SELECT * FROM player WHERE online = true;",
    ));
}

#[derive(Debug, Default, Resource)]
pub struct OnlineCounter(pub i32);

impl OnlineCounter {
    pub fn inc(&mut self) {
        self.0 += 1;
    }

    pub fn dec(&mut self) {
        self.0 -= 1;
    }
}

/// The connection proof: log the online player count whenever it changes.
fn report_players_on_player_inserted(
    mut messages: MessageReader<RowInserted<Player>>,
    mut counter: ResMut<OnlineCounter>,
) {
    if messages.is_empty() {
        return;
    }
    for _ in messages.read() {
        counter.inc();
    }
    info!("{} player(s) online", counter.0);
}

/// The connection proof: log the online player count whenever it changes.
fn report_players_on_player_deleted(
    mut messages: MessageReader<RowDeleted<Player>>,
    mut counter: ResMut<OnlineCounter>,
) {
    if messages.is_empty() {
        return;
    }
    for _ in messages.read() {
        counter.dec();
    }
    info!("{} player(s) online", counter.0);
}
