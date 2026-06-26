//! The SpacetimeDB Module: authoritative game state (tables) and the reducers that mutate it.
//!
//! Compiled to wasm and published with `just publish`. Client bindings are generated from it
//! into the `stdb_bindings` crate via `just generate`.

use spacetimedb::{Identity, ReducerContext, Table, reducer, table};

/// One connected player. `public` so clients may subscribe to it.
#[table(accessor = player, public)]
pub struct Player {
    #[primary_key]
    identity: Identity,
    name: String,
    online: bool,
    #[auto_inc]
    color: u8,
}

#[table(accessor = cursor, public)]
pub struct Cursor {
    #[primary_key]
    id: Identity,
    x: f32,
    y: f32,
}

/// Runs once when the module is first published.
#[reducer(init)]
pub fn init(_ctx: &ReducerContext) {
    log::info!("monster-master module initialized");
}

/// Runs when a client establishes a connection. Marks them online (creating the row if new).
#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) {
    let identity = ctx.sender();
    if let Some(player) = ctx.db.player().identity().find(identity) {
        ctx.db.player().identity().update(Player {
            online: true,
            ..player
        });
    } else {
        ctx.db.player().insert(Player {
            identity,
            name: "anonymous".to_string(),
            online: true,
            color: 0,
        });
    }

    let _ = ctx.db.cursor().try_insert(Cursor {
        id: ctx.sender(),
        x: 0.0,
        y: 0.0,
    });
}

/// Runs when a client disconnects. Marks them offline.
#[reducer(client_disconnected)]
pub fn client_disconnected(ctx: &ReducerContext) {
    if let Some(player) = ctx.db.player().identity().find(ctx.sender()) {
        ctx.db.player().identity().update(Player {
            online: false,
            ..player
        });
    }

    let _ = ctx.db.cursor().id().delete(ctx.sender());
}

/// Lets a player set their display name. Proves a client→server reducer call round-trips.
#[reducer]
pub fn set_name(ctx: &ReducerContext, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    match ctx.db.player().identity().find(ctx.sender()) {
        Some(player) => {
            ctx.db.player().identity().update(Player {
                name: name.to_string(),
                ..player
            });
            Ok(())
        }
        None => Err("no player for this connection".to_string()),
    }
}

#[reducer]
pub fn set_cursor_position(ctx: &ReducerContext, x: f32, y: f32) {
    ctx.db.cursor().id().update(Cursor {
        id: ctx.sender(),
        x,
        y,
    });
}
