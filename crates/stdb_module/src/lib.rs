//! The SpacetimeDB Module: authoritative game state (tables) and the reducers that mutate it.
//!
//! Compiled to wasm and published with `just publish`. Client bindings are generated from it
//! into the `stdb_bindings` crate via `just generate`.

use spacetimedb::{reducer, table, Identity, ReducerContext, Table};

/// One connected player. `public` so clients may subscribe to it.
#[table(accessor = player, public)]
pub struct Player {
    #[primary_key]
    identity: Identity,
    name: String,
    online: bool,
}

/// Runs once when the module is first published.
#[reducer(init)]
pub fn init(_ctx: &ReducerContext) {
    log::info!("monster-master module initialized");
}

/// Runs when a client establishes a connection. Marks them online (creating the row if new).
#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) {
    let id = ctx.sender();
    if let Some(player) = ctx.db.player().identity().find(id) {
        ctx.db.player().identity().update(Player {
            online: true,
            ..player
        });
    } else {
        ctx.db.player().insert(Player {
            identity: id,
            name: "anonymous".to_string(),
            online: true,
        });
    }
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
