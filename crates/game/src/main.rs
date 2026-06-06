use std::sync::mpsc::Sender;

use bevy::prelude::*;
use spacetimedb_sdk::{DbContext, Table};
use stdb_bevy::{StdbConnection, StdbPlugin};
use stdb_bindings::{DbConnection, PlayerTableAccess};

// Baked in at build time (the `just` recipes load these from .env). Defaults target a local server.
const SPACETIMEDB_URI: &str = match option_env!("SPACETIMEDB_URI") {
    Some(v) => v,
    None => "http://127.0.0.1:3000",
};
const SPACETIMEDB_MODULE: &str = match option_env!("SPACETIMEDB_MODULE") {
    Some(v) => v,
    None => "monster-master",
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Monster Master".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(StdbPlugin { connect, tick })
        .add_systems(Startup, setup)
        .add_systems(Update, report_players)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Builds and initiates the SpacetimeDB connection, delivering it through `tx`. `build()` is
/// synchronous on native but async on wasm (connecting in the browser is async), so the two
/// targets diverge here — the rest of the app is identical.
fn connect(tx: Sender<DbConnection>) {
    let builder = DbConnection::builder()
        .with_uri(SPACETIMEDB_URI)
        .with_database_name(SPACETIMEDB_MODULE)
        .on_connect(|ctx, identity, _token| {
            info!("connected to SpacetimeDB as {identity:?}");
            ctx.subscription_builder()
                .on_applied(|_ctx| info!("subscription applied"))
                .on_error(|_ctx, err| error!("subscription error: {err}"))
                .subscribe(["SELECT * FROM player"]);
        })
        .on_connect_error(|_ctx, err| error!("SpacetimeDB connect error: {err}"))
        .on_disconnect(|_ctx, err| warn!("SpacetimeDB disconnected: {err:?}"));

    #[cfg(not(target_arch = "wasm32"))]
    match builder.build() {
        Ok(conn) => {
            let _ = tx.send(conn);
        }
        Err(err) => error!("SpacetimeDB build failed: {err}"),
    }

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async move {
        match builder.build().await {
            Ok(conn) => {
                let _ = tx.send(conn);
            }
            Err(err) => error!("SpacetimeDB build failed: {err}"),
        }
    });
}

/// Pumps the connection once per frame: applies queued messages and fires callbacks.
fn tick(conn: &DbConnection) {
    if let Err(err) = conn.frame_tick() {
        error!("frame_tick failed: {err}");
    }
}

/// The connection proof: log the online player count whenever it changes.
fn report_players(conn: Option<NonSend<StdbConnection<DbConnection>>>, mut last: Local<i64>) {
    let Some(conn) = conn else { return };
    let online = conn.0.db().player().iter().filter(|p| p.online).count() as i64;
    if online != *last {
        *last = online;
        info!("{online} player(s) online");
    }
}
