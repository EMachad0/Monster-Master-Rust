use bevy::prelude::*;
use spacetimedb_sdk::DbContext;
use stdb_bevy::{
    RowDeleted, RowForwarder, RowInserted, SdkConnectionDriver, StdbConnection, StdbPlugin,
    TableRegistration, is_stdb_connected,
};
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
            StdbPlugin::new(SdkConnectionDriver {
                uri: "http://127.0.0.1:3000".try_into().unwrap(),
                database_name: "monster-master".to_string(),
                tick: DbConnection::frame_tick,
            })
            .add_tables([TableRegistration::new(forward_players)])
            .with_connect_on_startup(),
        )
        .init_resource::<OnlineCounter>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                report_players_on_player_inserted,
                report_players_on_player_deleted,
            )
                .run_if(is_stdb_connected),
        )
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
        .subscribe(["SELECT * FROM player WHERE online = true"]);
}

/// Re-runs on every (re)connect: forwards `player` row changes into
/// `RowInserted`/`RowUpdated`/`RowDeleted<Player>` messages. A named fn (rather than an inline
/// closure) so its concrete signature pins `C`/`R` — the closure form can't infer them.
fn forward_players(conn: &StdbConnection<DbConnection>, fwd: RowForwarder<Player>) {
    fwd.forward(&conn.db().player());
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
