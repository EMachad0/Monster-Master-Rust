# Bevy + self-hosted SpacetimeDB, web-first, with a hand-written bridge

We are building the client in **Bevy 0.18** targeting **web (wasm) and desktop**, against a
**self-hosted SpacetimeDB 2.x** backend, and we are writing our **own thin Bevy↔SpacetimeDB bridge
over `spacetimedb-sdk` 2.x** (the `stdb_bevy` crate) rather than using the community
[`bevy_spacetimedb`](https://crates.io/crates/bevy_spacetimedb) crate.

## Considered Options

- **Use `bevy_spacetimedb`** (the obvious choice a reader would expect). Rejected because, as of its
  latest release (0.7.2, Dec 2025), it pins `bevy ^0.17` and `spacetimedb-sdk ^1.11` (SpacetimeDB
  **1.x**), and it is desktop-only: it connects via `DbConnection::run_threaded`, which spawns an OS
  thread, with no `cfg(target_arch = "wasm32")` path. Browser wasm has no OS threads, so the crate
  cannot meet our day-one web requirement, and it would lock us to an older Bevy and an older
  SpacetimeDB major version.
- **Hand-written bridge over `spacetimedb-sdk` 2.4 (chosen).** The SDK ships a wasm path
  (`wasm-bindgen`/`web-sys`/`gloo-net`/`tokio-tungstenite-wasm`) and exposes a per-frame
  `frame_tick()` that advances the connection without a background thread — a natural fit for a Bevy
  system that runs on both wasm (tick per frame) and desktop (tick per frame or a thread). Costs us
  the convenience features `bevy_spacetimedb` would have provided, which we will re-implement
  incrementally as needed.

## Consequences

- We own the connection lifecycle, event plumbing, and version compatibility — more code, full
  control, and no dependency on a lightly-maintained third-party crate.
- The bridge stays module-agnostic; module-specific types live in the generated `stdb_bindings`
  crate.
- We must keep the SpacetimeDB **server**, **CLI**, and **`spacetimedb-sdk`** versions aligned
  (all 2.x) to avoid protocol mismatches.
