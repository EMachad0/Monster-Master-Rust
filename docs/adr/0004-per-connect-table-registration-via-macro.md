# Per-connect table registration via a macro, declared on `StdbPlugin`

The Bridge turns subscribed rows into buffered messages (`RowInserted<T>` /
`RowUpdated<T>` / `RowDeleted<T>`) by registering the SDK's `on_insert` / `on_update` /
`on_delete` callbacks on the live connection. A Game opts a table in by declaring it on the
plugin:

```rust
StdbPlugin::new(SdkConnectionDriver { .. })
    .add_tables([
        stdb_table!(player => Player),                 // all callbacks
        stdb_table!(monster => Monster, [insert, delete]), // subset / no-PK table
    ])
    .with_connect_on_startup();
```

`add_tables` accepts **many** tables (an array / iterator); `stdb_table!` expands to one opaque
registration value. Because we rebuild the connection on every reconnect (ADR 0003) and refuse
to `Box::leak` it (ADR 0002), registration is **re-run on every `StdbConnected`**, not once at
app-build — a connection built by a reconnect starts with an empty cache and *no* callbacks, so a
registration that ran only once would silently stop emitting row messages after the first drop.

## Considered Options

### How the Game names a table's callbacks

- **The reference crate's accessor function-pointer (`add_table(RemoteTables::player)`).** Generic
  over `F: Fn(&'static C::DbView) -> TTable` with `TTable: Table + TableWithPrimaryKey`. Rejected:
  the accessor *returns a handle that borrows the db view* (`PlayerTableHandle<'a>`), so its type
  only resolves generically when `'a = 'static` — which the reference obtains by **leaking the
  connection** (`Box::<C>::leak`). We refuse the leak (ADR 0002: a leak per reconnect is unbounded
  over a long, flaky-socket session), and `Table` is not object-safe (`on_insert` is generic over
  the callback), so there is no transient-borrow generic form (`for<'a> Fn(&'a DbView) -> Handle<'a>`
  is inexpressible, and `dyn Table` is impossible). The elegant reference API is bought *entirely*
  with the leak, which our reconnect-by-rebuild model has already nailed shut.
- **A per-table closure written by the Game** (`add_stdb_table(app, |db, sink| { .. })`). This *does*
  work with a transient borrow — `for<'a> Fn(&'a DbView, RowSink<T>)` returns `()`, so no
  lifetime-parameterised return type and no leak. Rejected as the *default surface*: it makes the
  Game hand-write the same three-line `db.player().on_insert(..)` registration plumbing for every
  table.
- **A macro generating that closure (chosen).** `stdb_table!(player => Player)` expands to the
  concrete, transient-borrow registration plus the channel/message/drain wiring. Concrete inline
  code sidesteps the lifetime wall (the borrow is just the natural borrow of the expression — no
  generic handle type is ever named), and the Game writes no plumbing.

### Where registrations live, and whether they are dynamic

- **As ECS entities the Game can spawn/despawn mid-game.** Rejected: a table's message pipeline
  (`RowInserted<Player>`, …) is a compile-time-typed, static fact — you know your table *types* at
  build. The genuinely dynamic axis is the **Subscription** ("what data am I fetching"), not table
  registration ("this table exists and I want its callbacks"). Toggling a table's callbacks at
  runtime changes nothing about data flow (no subscription → no rows → no callbacks fire anyway), so
  it buys ~nothing while adding an entity lifecycle to reason about.
- **Declared once on the plugin, re-registered each connect (chosen).** `add_tables` records the
  registrations on the plugin; `build()` installs, for each table, its `RowChannel<T>` + messages +
  drain system, and an observer that re-registers that table's callbacks on every `StdbConnected`.

## Consequences

- Re-registration on **every** connect is load-bearing: a reconnect that failed to re-register would
  silently stop emitting row messages while looking healthy.
- Callback **selection** and **no-PK tables** fall out of *which* callbacks the macro emits (the
  `[insert, delete]` list) — no `TableWithPrimaryKey` bound gymnastics and no separate
  `add_table_without_pk` entry point like the reference needs.
- **Tables are static; Subscriptions are dynamic.** Table registration is fixed at plugin
  construction. Choosing/changing *what rows flow in* stays a runtime concern of the Game's
  subscription (issued in the `StdbConnected` observer, per ADR 0003 / `docs/spacetimedb.md`).
- The typed engine — `add_stdb_table::<C, T>(app, register_fn)` (channel + messages + drain + the
  per-connect observer) — is plain Rust and **unit-testable** with a fake `register_fn` that counts
  invocations across connect/reconnect, no socket. Only the macro's concrete
  `db.player().on_insert(..)` body is manual/integration-verified.
- `StdbPlugin` now carries a list of boxed registrations, so it is no longer `Copy`.
- The macro names **no connection type**: `C` is inferred backward from `add_tables`. Making that
  work pins two implementation choices:
  - `TableRegistration<C>` carries `C` as a `PhantomData` type parameter (not erased into the boxed
    closure), so the registration's type exposes `C`.
  - `add_tables` takes a **concrete const-generic array** `[TableRegistration<Cd::Conn>; N]`, not
    `impl IntoIterator`. Only the concrete element type pins `C` early enough for a bare
    `|conn, fwd|` closure to infer it (verified: `impl IntoIterator` leaves the closure params
    unconstrained → `type annotations needed`). Trade-off: `add_tables` accepts array literals
    only — no `Vec`/iterator — which suits a static, compile-time table set.
  - `R` needs no annotation either: it is inferred from the accessor's row type via the
    `RowForwarder` method bound.
