# stdb_bevy bridge: a `Send` connection in a `Res`, a per-frame `frame_tick` pump, and no leaked connection

We are implementing the `stdb_bevy` **Bridge** (see ADR 0001 for why it is hand-written rather than
`bevy_spacetimedb`). Its public API is modelled on `bevy_spacetimedb` — a configured `StdbPlugin`, a
`StdbConnection` resource exposing `db()` / `reducers()` / `subscription_builder()`, connection
lifecycle signals, and typed per-table row-change signals — but it targets **SpacetimeDB SDK 2.4,
Bevy 0.18, and web (wasm) as a first-class target**, which forces three departures from the
reference's *implementation*.

## Considered Options

### How to advance the connection

- **`DbConnection::run_threaded` (what `bevy_spacetimedb` uses).** Spawns an OS thread that receives
  websocket messages. Rejected: there is no `run_threaded` on wasm — the browser has no OS threads.
- **`frame_tick` once per frame from a Bevy system (chosen).** The SDK exposes `frame_tick`, which
  drains all queued messages and fires callbacks without a background thread. One code path drives
  both native and wasm. This is the core reason the Bridge is hand-written.

### `Res` vs `NonSend` for the connection

- **`NonSend` resource (our own earlier assumption, and what the old docs claimed).** Motivated by the
  belief that "the SDK's browser connection types are not `Send`." Rejected: **this is false.** A
  compile-time `Send + Sync` assertion on `DbConnection` compiles on *both* the native (tokio) and
  browser (`web-sys` / `gloo` / wasm32) code paths. The SDK confines the only `!Send` types (the
  browser socket) to a `spawn_local` background task; the handle itself holds only `Arc<Mutex<…>>`
  state and `Send`-bounded callbacks. Storing it `NonSend` would needlessly pin every reducer-calling
  or subscribing system to the main thread for no benefit.
- **A normal `Res<StdbConnection<C>>` (chosen).** Because the handle is `Send + Sync`, the connection
  lives in an ordinary resource and every Game system is an ordinary multi-threaded system — matching
  the reference's ergonomics. We keep the `frame_tick`-per-frame pump regardless; `Send`-ness only
  decides the *resource kind*, not the *threading model*.

### How the Game registers tables for row-change messages

- **`add_table(RemoteTables::player)` via `Box::leak` (what `bevy_spacetimedb` does).** The table
  accessor returns a handle that borrows the connection (`PlayerTableHandle<'a>`); to name it as one
  generic type the reference pins the lifetime to `'static` and leaks the connection with `Box::leak`.
  Rejected: we **auto-reconnect by building a new connection** on every disconnect, so leaking would
  strand a whole connection — and its client cache — in memory on every network drop.
- **Transient-borrow registration behind a macro (chosen).** Registering a callback
  (`handle.on_insert(cb)`) only stores the boxed callback in the cache's `Arc<Mutex<…>>` and returns;
  nothing keeps borrowing the handle afterward, so registration needs only a short-lived `&C` borrow —
  no `'static`, no leak. The `stdb_tables! { player => Player }` macro expands to this concrete,
  transient-borrow registration, keeping the Bridge module-agnostic (it never names a Module type) and
  leak-free across reconnects.

## Consequences

- The Bridge owns the connection lifecycle: it hides the native-sync /
  wasm-`spawn_local(build().await)` split, pumps `frame_tick` each frame, **auto-reconnects with
  backoff**, and **reuses the connection token in-memory** so a player keeps the same `Identity` across
  a same-session reconnect. Cross-session token persistence is deliberately out of scope for v1.
- Lifecycle is delivered as **observer events** (`StdbConnected` / `StdbDisconnected` /
  `StdbConnectionError`, fired on every (re)connect); row changes as **buffered messages**
  (`RowInserted<T>` / `RowUpdated<T>` / `RowDeleted<T>`). A `stdb_connected` run condition gates systems
  that need a live connection; a `Res<StdbStatus>` exposes the current state for UI.
- The SDK does **not** clear the client cache or emit deletes on disconnect (`subscription.rs`
  `on_disconnect` is a no-op), and because we rebuild the connection on reconnect (fresh, **empty**
  cache per `build()`) the re-applied subscription **re-fires `on_insert` for every still-matching
  row** — not updates, *inserts* (verified in SDK 2.4: `client_cache.rs` builds a fresh cache per
  `build()`; subscription apply fires inserts). The Bridge therefore **does not synthesize deletes on
  disconnect** — an earlier revision of this ADR mandated a blanket delete-sweep; we reversed it.
  Rationale: a sweep makes every row that *persists* across a blip despawn-then-respawn (visible
  flicker), and it bakes one reconciliation policy into a module-agnostic forwarder. Instead the
  Bridge forwards **only real SDK row events**, plus the lifecycle observer events as hooks, and
  **reconciliation across reconnect is the Game's policy**:
  - Key entities by primary key and treat `RowInserted` as an **upsert** (find-or-spawn by pk, then
    overwrite components). Reconnect re-inserts then refresh existing entities — no flicker, no
    duplicates. (`RowUpdated` shares the same "reflect this row's state" path; `RowDeleted` despawns.)
  - A Game that prefers clean-slate-on-disconnect can despawn its entities in a one-line
    `StdbDisconnected` observer (opt-in sweep) — the Bridge leaves the choice to the Game rather than
    forcing it.
  - Known gap: a row deleted *during* an outage is never signalled (the reconnect dump only
    re-inserts still-matching rows), so it lingers as a ghost. Fix when felt, via a resync
    mark-and-sweep (mark stale on reconnect, clear on each refreshed row, despawn the still-stale) or
    a future bridge-side PK reconciliation.
- We own more code than the reference (lifecycle, reconnection, the registration macro), but gain a
  single native+wasm path, no leaks under reconnection, and full `Res`-based ergonomics.
- Reducer-outcome events and cross-session token persistence are known near-term follow-ups, not v1.
