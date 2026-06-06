//! Minimal, module-agnostic bridge between a SpacetimeDB connection and Bevy.
//!
//! This crate knows nothing about any specific Module. It:
//!   - receives the connection through a channel (built synchronously on native, or after an async
//!     `build().await` on wasm — connecting in the browser is inherently asynchronous),
//!   - stores it as a `NonSend` resource (the SDK's browser connection types are not `Send`), and
//!   - pumps it once per frame via a `tick` function the Game supplies.
//!
//! `frame_tick` is an inherent method on the *generated* `DbConnection`, not a trait method, so the
//! Game passes `connect`/`tick` as plain `fn` pointers. That also keeps `StdbPlugin<C>` `Send + Sync`
//! regardless of `C`.
//!
//! The library niceties (typed per-table insert/update/delete events, macros) that a full
//! integration would provide are intentionally out of scope for now.

use std::sync::mpsc::{channel, Receiver, Sender};

use bevy::prelude::*;

/// The live SpacetimeDB connection. Generic over the concrete generated `DbConnection`.
pub struct StdbConnection<C: 'static>(pub C);

/// Holds the receiver until the connection is delivered (immediate on native, async on wasm).
struct PendingConnection<C: 'static>(Receiver<C>);

/// Wires a SpacetimeDB connection into a Bevy `App`:
/// - `connect` is handed a [`Sender`] and must deliver the built connection through it. On native,
///   build synchronously and `send`. On wasm, `spawn_local` the async `build().await` and `send`
///   from inside it.
/// - `tick` pumps the connection every `Update` (call the connection's `frame_tick`).
pub struct StdbPlugin<C: 'static> {
    pub connect: fn(Sender<C>),
    pub tick: fn(&C),
}

impl<C: 'static> Plugin for StdbPlugin<C> {
    fn build(&self, app: &mut App) {
        let connect = self.connect;
        let tick = self.tick;

        app.add_systems(Startup, move |world: &mut World| {
            let (tx, rx) = channel::<C>();
            connect(tx);
            world.insert_non_send_resource(PendingConnection(rx));
        });

        app.add_systems(Update, move |world: &mut World| {
            // Promote the pending connection to a live resource once it has been delivered.
            if world.get_non_send_resource::<StdbConnection<C>>().is_none() {
                let received = world
                    .get_non_send_resource::<PendingConnection<C>>()
                    .and_then(|pending| pending.0.try_recv().ok());
                if let Some(conn) = received {
                    world.insert_non_send_resource(StdbConnection(conn));
                    world.remove_non_send_resource::<PendingConnection<C>>();
                }
            }

            if let Some(conn) = world.get_non_send_resource::<StdbConnection<C>>() {
                tick(&conn.0);
            }
        });
    }
}
