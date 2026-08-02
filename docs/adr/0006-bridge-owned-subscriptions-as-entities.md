---
status: accepted (supersedes the "Subscription is a Game concern" stance of ADR 0003)
---

# Bridge-owned Subscriptions modelled as ECS entities

The Game declares a **Subscription** by spawning an entity carrying a `Subscription(Box<[String]>)`
component; the **Bridge** owns the lifecycle — it issues each query set to the SDK, re-issues **all**
of them on every (re)connect, and unsubscribes on despawn. This supersedes ADR 0003's stance that the
Subscription is the Game's concern: the Bridge must own it to re-apply across reconnects (the SDK has
no resume — a reconnect rebuilds the connection with an empty cache and no subscriptions, per ADR
0003/0004) and to expose the **Resync fence** that the later Resync work needs.

## Considered Options

### Where the subscription lives: ECS entities vs plugin-static

- **Plugin-static (like table registration, ADR 0004).** Rejected: ADR 0004 already fixed the axis —
  *tables are static (you know your table types at build); the genuinely dynamic axis is the
  Subscription (what data am I fetching, changeable at runtime)*. Static declaration can't express
  subscribe/unsubscribe mid-game.
- **ECS entities (chosen).** One entity = one query set. Spawn = subscribe, despawn = unsubscribe.
  Multi-set falls out (N entities = N sets, including several over the same table — impossible with a
  type-keyed model). The Game declares subs at `Startup` (or any time); the Bridge issues them on
  connect, so no on-connect observer is needed in the Game. The Game owns the entity's *lifecycle*;
  the Bridge owns only the marker/token components it attaches.

### Where the module-specific `subscribe()` call lives, and how the plugin stays optional

`DbContext::subscription_builder()` returns an **unbounded** associated type (`type
SubscriptionBuilder;`, no trait bound), so generically over `C: DbContext` it is opaque with zero
methods; `.on_applied().subscribe(..)` exist only on the concrete `SubscriptionBuilder<M>`. The SDK
adapter recovers the methods by pinning the associated type with an equality bound:
`C: DbContext<SubscriptionBuilder = __codegen::SubscriptionBuilder<M>>`. The generic engine still
**cannot** issue a subscribe.

- **A macro, like `stdb_table!`.** Rejected: the query is *runtime data* (a `String` on an entity),
  not a compile-time table type — a macro has nothing runtime to bind.
- **On the existing `StdbConnectionDriver` trait.** Rejected: bundling `subscribe`/`unsubscribe`
  onto the one driver couples the two concerns — a fake could no longer exercise connection
  behaviour *without* also satisfying subscriptions, so connection and subscription behaviour can't
  be tested in isolation.
- **A separate `StdbSubscriptionDriver` trait (chosen).** `subscribe`/`unsubscribe`/`clear` live on
  their own trait (own `type Conn`); the SDK implements it on a **separate `SdkSubscriptionDriver`
  struct** — distinct from `SdkConnectionDriver` because the two own different connection-scoped state
  (uri/token vs the `SubscriptionId → handle` map), so a single both-traits SDK struct would fuse
  unrelated state. Both touch `M` concretely (the `__codegen` touch already contained there per ADR
  0003). A Game's escape-hatch driver uses the *public* generated `subscription_builder()`. A driver
  *may* still implement **both** traits on one struct — the `FakeDriver` does — but the SDK's are two.
  Fakes implement only the trait they need — a connection fake stays subscription-free — keeping the
  two layers independently unit-testable with no socket.

The split lets `StdbPlugin` carry a connection driver and a subscription driver of independent
types. Subscriptions are **on by default**: fetching data is the common case, so the ergonomic
constructors enable them and a non-subscribing setup is the explicit exception.

Neither driver is `Clone`. A connection driver holds config plus a shared token; a subscription
driver holds a live `SubscriptionId -> handle` map and a monotonic id counter, neither of which has
coherent copy semantics (an earlier `Clone` that zeroed the counter while copying the map was a
latent corruption trap). But `Plugin::build` takes `&self` and cannot move an owned field into a
resource. So the plugin owns a **builder** rather than the drivers: the `StdbBuilder` trait (with
associated `Cd`/`Sd` and `Sd::Conn = Cd::Conn` pinned) exposes `build_cd`/`build_sd`, which produce
a fresh driver at build time, inserted by move. No `Clone`, no interior mutability, and because the
construction lives in the concrete builder the generic `build` never names a concrete driver type.

Every plugin now carries a subscription driver, so there is **one** `Plugin` impl for
`StdbPlugin<B, Cd, Sd>`, not the earlier pair of non-overlapping impls. Connection-only setups use a
no-op driver instead of a separate code path: `NoSubscriptions<C>` **implements**
`StdbSubscriptionDriver` with no-op `subscribe`/`unsubscribe`/`clear` (it was previously a
non-implementing marker kept only for coherence). Its systems are wired but inert while no
`Subscription` entity exists, so connection behaviour still runs without a real subscription driver.

Constructors:

- **`new(builder)`** — the general entry: install any `StdbBuilder`.
- **`sdk(uri, database_name, tick)`** — the SDK convenience: wraps an `SdkBuilder` that builds an
  `SdkConnectionDriver` and an `SdkSubscriptionDriver` over the same `M`/`C`, so a Game never names
  either SDK driver. Subscriptions on.
- **`connection(conn_driver)`** — subscriptions off: wraps the pre-built driver alongside
  `NoSubscriptions` in a `Drivers` builder. For connection-only tests/fakes and Games that never
  subscribe.
- **`Drivers::new(conn_driver, sub_driver)`** — a builder over two already-built drivers, each
  `Clone` and cloned on build; the way a both-traits fake is installed as two instances of the same
  type. Passed to `new`. `Sd::Conn = Cd::Conn` is enforced by the `StdbBuilder` trait, so the shared
  connection type is checked at compile time.

The SDK drivers are built from parameters, so they need no `Clone`; `Drivers` requires `Clone` and
serves pre-built drivers that are cheap to clone, chiefly the Arc-sharing test doubles. A full
typestate `builder().with_connection(..).with_subscription(..).build()` stays unnecessary: one
`StdbBuilder` value already carries any construction recipe behind a single type.

### Keeping the subscription handle

`driver.subscribe` yields `M::SubscriptionHandle` (module-specific, `Clone + Send + 'static`).
Dropping a handle does **not** unsubscribe — the SDK only unsubscribes on an explicit `unsubscribe()`
(verified: no unsubscribing `Drop`); skipping it would leave the rows in the cache and never despawn
the row entities. So the handle must be kept and `unsubscribe()` called explicitly on despawn.

The handle is module-specific, but the entity is **non-generic**, so a concrete handle cannot live on
the entity — you must either erase it onto the entity (no map, but `Box<dyn Any>`/`Box<dyn Trait>`) or
keep it concrete in the driver behind a non-`Entity` id (a map). We keep it **in the driver**:

- The **driver owns** a `HashMap<SubscriptionId, M::SubscriptionHandle>` and mints ids from a plain
  `u64` counter: connection-scoped SDK state belongs in the SDK adapter, not on a Game-owned entity.
  The subscription systems reach it through `ResMut`, whose exclusive access removes any need for
  `Arc`/`Mutex` or a `Clone` impl (the driver is built once and installed by move). So the **driver
  never names `Entity`** (no ECS coupling) and there is **zero type erasure** anywhere (the handle
  stays concrete).
- `subscribe(..) -> SubscriptionId` stores the handle and returns the id; the reconcile puts the id on
  the entity as the `IssuedSubscription { id }` component — a plain Copy **data** id, never behaviour
  (mirrors Bevy's `Handle`/`Assets` split, but with our own id since `Handle<T>` is asset-only).
- `unsubscribe(id)` (driver method, called from the `On<Remove, Subscription>` observer) does the SDK
  `unsubscribe()` — **the component never carries the unsubscribe behaviour.** This is what an earlier
  `IssuedSubscription { handle: Box<dyn SubscriptionHandle> }` got wrong (behaviour parked in a
  component; a transient bridge handle co-located with the Game's durable intent).
- **No `Drop`-based unsubscribe** and **no unsubscribe on disconnect/reconnect.** Only a real despawn
  (`On<Remove, Subscription>`) unsubscribes. On disconnect the reset clears the driver's map (the
  handles are dead — dropped, never unsubscribed); a `Drop` impl would fire spurious unsubscribes on
  dead handles, and `mem::forget`-ing to suppress it would leak per sub per reconnect (the
  unbounded-leak failure ADR 0002 fought). Observable state comes from the `on_applied`/`on_error`
  channel, never from polling the handle.

### Per-entity state: marker components vs an enum

- **A `SubscriptionState` enum component.** Rejected: Bevy filters at the *archetype* level
  (`With`/`Without`); an enum forces `Query<&State>` + a per-entity `match` and no cheap `is_empty()`
  fence.
- **Marker components (chosen).** `AppliedSubscription` (bare) and `FailedSubscription`, plus
  `IssuedSubscription { id }` (the "issued this connection" marker, carrying the driver's
  `SubscriptionId`). Derived states are pure filters; the `Resync fence` is `Query<(),
  (With<Subscription>, Without<AppliedSubscription>, Without<FailedSubscription>)>.is_empty()`, exposed
  as the `is_subscriptions_settled` run-condition. Exclusivity is structural: the SDK fires exactly
  one of `on_applied`/`on_error` per set per connection, and `On<StdbDisconnected>` strips all three
  bridge markers so every sub restarts clean.

### One query string vs a set: `String` vs `Box<[String]>`

- **`Subscription(String)`.** Simpler, multi-set via multiple entities — but loses **atomic
  grouping**: the SDK's `subscribe([...])` applies a whole set under one `on_applied`, so related
  queries can be made consistent at one snapshot boundary.
- **`Subscription(Box<[String]>)` (chosen).** Faithful 1:1 mirror of the SDK's subscription unit
  (one set = one `on_applied`/`on_error`), keeping entity-state ↔ callback exactly 1:1. The
  single-query case stays terse via constructors `query("…")` and `table("player")` (→ `SELECT * FROM
  player`).

### Error policy: subscription error ≠ connection drop

A subscription's `on_error` is the **host rejecting the query** (`Error::SubscriptionError`,
*"Host returned error when processing subscription query"*) — almost always **permanent** (bad SQL,
missing/non-`public` table or column, type error, limits). Therefore: **no auto-retry.** Reconcile
never re-issues an issued sub (it keeps `IssuedSubscription`, so the `Without<IssuedSubscription>`
filter skips it — no tight loop spamming a rejected query); a reconnect clears markers and gives
**one** fresh attempt per reconnect (transient errors self-heal, permanent ones log once and stop).
This is the deliberate opposite of a *connection* drop, which is transient and *does* back-off-retry
(ADR 0003). The Bridge marks `FailedSubscription` and fires `SubscriptionFailed` rather than
despawning — it respects the Game's ownership of the entity, and the marker keeps the Resync fence
from hanging on an unfixed query.

### Typed, compile-time-checked subscriptions — deferred, and one dead end recorded

A typed `Subscription::all::<T>()` tying the row type to its table name is **impossible**: the impl
would be a foreign trait on a foreign type in `game` (orphan rule), can't live in the generated
`stdb_bindings` (must stay Bevy-agnostic) nor in the module-agnostic `stdb_bevy`. sqlx-style
checking has no STDB analogue, and the SDK's own typed query builder
(`SubscriptionBuilder::add_query<T, Q: Query<T>>`) is unstable and not emitted by `spacetime
generate` for our tables. Deferred — but **unblocked**: because the driver owns `subscribe`, a typed
path can be added there later with no change to the entity model.

## Consequences

- New public surface: `Subscription` (+ `new`/`query`/`table`), the `AppliedSubscription` /
  `FailedSubscription` / `IssuedSubscription` markers,
  `SubscriptionApplied`/`SubscriptionFailed`/`SubscriptionUnsubscribed` entity-targeted observer
  events, and the `is_subscriptions_settled` run-condition. The driver's `SubscriptionId`→handle map
  is its own private state.
- A separate `StdbSubscriptionDriver` trait carries `subscribe`/`unsubscribe`/`clear`; the SDK
  implements it on `SdkSubscriptionDriver`, a struct distinct from `SdkConnectionDriver`. Neither SDK
  driver is `Clone`; the plugin builds each from an `StdbBuilder` and inserts it by move.
  `StdbPlugin<B, Cd, Sd>` has one `Plugin` impl and these constructors: `new(builder)` (any builder),
  `sdk(uri, database_name, tick)` (subscriptions on, wraps `SdkBuilder`), and `connection(driver)`
  (subscriptions off, wraps the driver with the no-op `NoSubscriptions` in a `Drivers` builder).
  Pre-built driver pairs go through `Drivers::new(conn_driver, sub_driver)`. Connection-only test
  fakes go through `connection`, so connection behaviour is still exercised without a real
  subscription driver.
- The reset→reconcile loop unifies initial-subscribe and reconnect-re-issue into one path:
  `On<StdbDisconnected>` strips the bridge markers from every subscription (the rebuilt connection's
  SDK handles are dead, so the markers are dropped, never unsubscribed), leaving the subs *pending*;
  the reconcile system (run while connected) then re-issues every pending sub on the next connect.
  Stripping on **disconnect** rather than reconnect keeps the world state truthful during the outage
  (a disconnected sub is not "applied") and makes the Resync fence read `false` the instant the
  socket drops, with no stale-`Applied` window.
- A clean unsubscribe reuses the existing `RowDeleted<T>` forwarder: `unsubscribe()` sends
  `SendDroppedRows`, the server's `UnsubscribeApplied` carries the dropped rows as deletes through the
  same `apply_update` path, firing `on_delete`. It is **asynchronous** (deletes arrive a later
  `frame_tick`) and only fires on a live socket.
- This builds only the **foundation + the Resync fence seam**. Resync itself (mark-stale, generation
  stamps, ghost sweep, `TablePk`/PK index) is explicitly out of scope and lives in a later session;
  `is_subscriptions_settled` is the one hook it consumes.
