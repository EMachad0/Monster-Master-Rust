---
status: accepted (supersedes the original ADR 0013 "bridge clean architecture refactor", deleted with its branch, and the issue #26 migration plan)
---

# The SDK boundary: an SDK-free core enforced by an adapter crate split

The Bridge splits into two crates: `stdb_bevy`, the core, which drops the `spacetimedb-sdk`
dependency entirely, and `stdb_bevy_sdk`, the adapter that binds the core's seams to the SDK.
The boundary rule is **type-level**: no `spacetimedb_sdk` or `__codegen` path may be referenced
outside the adapter crate. Enforcement is the split itself: dependency absence makes a stray SDK
reference in core a build error, not a lint. This ADR records the rule, the verdict for every
coupling site found in the core, and the target API shape of each fix. Implementing the fixes is
a separate effort; the identity and row-path shapes were prototyped and compile-checked
(`just check::all` green) before being accepted.

## The boundary rule

- **Type-level, grep-able.** Outside the adapter crate, no code names `spacetimedb_sdk` or
  `__codegen` paths: no imports, no trait bounds, no re-exports, no signatures.
- **Shape-level coupling is weighed per site, not banned wholesale.** A contract that mirrors an
  SDK idiom without naming SDK types can still chase the SDK's reshapes; where that chase was
  found (the reducer callback shape), the shape moved inside the fence too, so core's public API
  freezes across SDK bumps.
- **Why:** an SDK version migration's blast radius becomes the adapter crate alone. Core does not
  even recompile against a reshaped SDK, and every engine-layer test runs against SDK-free fakes.
- The Bridge's **Bevy** coupling is accepted and stays; removing it was judged a net downgrade.
  So is business logic living in Bevy systems, tested through an `App`. This rule fences the SDK
  only.

## Verdicts

| Coupling site | Verdict | Resolution |
| --- | --- | --- |
| `StdbIdentity(pub sdk::Identity)` + lifecycle `Identified` | fix | Bridge-owned opaque 32-byte newtype; SDK converted once at the adapter seam ([#28](https://github.com/EMachad0/Monster-Master-Rust/issues/28)) |
| `StdbBevyError::SdkError(#[from] sdk::Error)` | fix | Bridge-owned `Driver(Arc<dyn Error + Send + Sync>)` catch-all ([#29](https://github.com/EMachad0/Monster-Master-Rust/issues/29)) |
| `StdbPlugin::sdk` constructor (tick `sdk::Result`, `DbContext` bounds) | fix | signature unchanged, relocated behind `SdkPluginExt` in the adapter ([#29](https://github.com/EMachad0/Monster-Master-Rust/issues/29), [#32](https://github.com/EMachad0/Monster-Master-Rust/issues/32)) |
| Reducer sink `cb` naming `__codegen::InternalError` | fix | core takes Bridge-owned `ReducerOutcome`; SDK-shaped closure via `SdkReducerSinkExt` in the adapter ([#30](https://github.com/EMachad0/Monster-Master-Rust/issues/30)) |
| `RowForwarder` bound to `sdk::table::With*` | fix | Bridge-owned capability traits + blanket SDK adapters; forwarder stays in core ([#31](https://github.com/EMachad0/Monster-Master-Rust/issues/31)) |
| Registration importing `sdk_impl` (`bsatn_key`, `Serialize`) | fix | keyless path deleted outright; the import disappears with it ([#31](https://github.com/EMachad0/Monster-Master-Rust/issues/31)) |
| Test fakes implementing SDK table traits | fix | fakes implement the Bridge traits directly, zero SDK references ([#31](https://github.com/EMachad0/Monster-Master-Rust/issues/31)) |
| `pub use spacetimedb_sdk as __sdk` feeding `stdb_table!` | fix | deleted; the macro expands SDK-free via `$crate::DbAccess` / `$crate::RowCollection` ([#31](https://github.com/EMachad0/Monster-Master-Rust/issues/31)) |
| Raw `DbContext<SubscriptionBuilder = ..>` plugin bound | dissolved | no replacement trait; the bound relocates into the adapter with the `sdk` constructor ([#32](https://github.com/EMachad0/Monster-Master-Rust/issues/32)) |

No named accepted seams: the exemption list is empty, and the fence is exactly the adapter crate.

### Identity

`StdbIdentity` becomes a Bridge-owned opaque newtype; the SDK type leaves core entirely.

```rust
// stdb_bevy core: no SDK reference
#[derive(Resource, Deref, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdbIdentity([u8; 32]);

impl StdbIdentity {
    pub fn new(bytes: [u8; 32]) -> Self { Self(bytes) }
}

impl PartialEq<[u8; 32]> for StdbIdentity { /* byte compare */ }
```

`LifecycleEvent::Identified(StdbIdentity)` and `LifecycleSink::identified` take the newtype; the
adapter converts once, in `on_connect`: `sink.identified(StdbIdentity::new(identity.to_byte_array()))`.
The Game answers "is this mine?" via `*identity == player_identity.to_byte_array()`.

Comparing against raw bytes needs no `PartialEq` across two foreign types, so the old orphan-rule
wall dissolves rather than being worked around. An associated `type Identity` on the connection
contract was rejected: it kills the blanket `StdbConn` impl and makes the identity resource
generic over the connection. Keeping the SDK type as a named seam was rejected: it would hold the
SDK dependency in core for a single value type, foreclosing the crate split.

### Errors

`StdbBevyError` drops the SDK-naming variant for a Bridge-owned catch-all:

```rust
pub enum StdbBevyError {
    ConnectionRefused,
    Driver(Arc<dyn std::error::Error + Send + Sync>),
}
```

`#[error(transparent)]` keeps today's Display output; `Arc` preserves `Clone`, `Debug`, and the
source chain (a consumer that needs the concrete cause can downcast). The `#[from]` conversion
disappears; the adapter maps explicitly at its four call sites.

**Growth policy:** a semantic variant (e.g. `AuthRejected`) is added only when (1) a real Game
consumer needs the distinction and (2) the SDK failure mode has been observed empirically. The
SDK's own error enum types most causes as message strings, so typing finer than the SDK means
parsing strings, and mirroring its variants 1:1 is shape coupling that every SDK bump revisits.
The same policy governs `ReducerOutcome` below.

The tick signature `fn(&C) -> spacetimedb_sdk::Result<()>` survives unchanged: the offense was
location, not type. The `StdbPlugin::sdk` constructor moves behind an extension trait in the
adapter, mirroring the existing `SyncAppExt` pattern:

```rust
pub trait SdkPluginExt: Sized {
    type Conn;
    fn sdk<U>(
        uri: U,
        database_name: impl Into<String>,
        tick: fn(&Self::Conn) -> spacetimedb_sdk::Result<()>,
    ) -> Self
    where
        U: TryInto<http::Uri>,
        U::Error: Debug;
}
```

The Game call site stays `StdbPlugin::sdk(uri, name, DbConnection::frame_tick)` plus one trait
import. An extension trait survives the crate split unchanged (local trait, foreign type); an
inherent impl must live in the defining crate. The tick error never crosses into core: the SDK
driver logs it and drops it.

### Reducer sink

Core drops the SDK callback shape entirely; `ReducerOutcomeSink::cb` takes a Bridge-owned outcome
that reifies the existing glossary term:

```rust
pub enum ReducerOutcome {
    Committed,
    Failed(String),
}

impl ReducerOutcomeSink {
    pub fn cb<K>(&self) -> impl FnOnce(ReducerOutcome) + Send + 'static
    where
        K: Send + Sync + 'static;
}
```

Two variants only: current behavior folds a host abort into `Failed`, and the growth policy above
gates any `Aborted` variant. The SDK-shaped closure comes from an extension trait in the adapter,
generic over the abort error so `__codegen::InternalError` (which carries no stability promise)
appears in no Bridge contract at all:

```rust
pub trait SdkReducerSinkExt {
    fn sdk_cb<K, Ctx, E>(&self) -> impl FnOnce(&Ctx, Result<Result<(), String>, E>) + Send + 'static
    where
        K: Send + Sync + 'static,
        E: std::fmt::Display;
}
```

Keeping a generic mirror of the callback shape in core was rejected: it removes the type reference
but leaves core's public API chasing the SDK's callback shape on every reshape. Moving the shape
into the adapter lands the chase where the drivers already chase.

### Row path

Core gets Bridge-owned capability traits, one per messages-mask field, plus the whole-table read
Resync diffs against: `RowInsertSource`, `RowDeleteSource`, `RowUpdateSource` (callbacks carry the
row payload only, no event context, no callback ids) and `RowCollection::rows()`. `DbAccess::db()`
is the macro's route to table accessors and sits beside `StdbConn`, not absorbed into it: absorbing
would kill `StdbConn`'s blanket impl and force a dummy `Db` type on every lifecycle-only test fake.
Each bound names only what its site uses.

The adapter provides blanket impls from the SDK's capability-shaped table traits and `DbContext`
onto the Bridge traits, so every generated handle satisfies the row-path bounds with zero per-table
code. This deliberately differs from the extension-trait shape used for the plugin and reducer
seams: extension traits would have kept the fakes and forwarder tests bound to SDK traits, while
blanket adapters make the whole engine-layer row path SDK-free. `RowForwarder` stays in core,
bound to the Bridge traits, unit-testable with SDK-free fakes (a fake must not also implement the
SDK table traits, or the blanket adapters would collide).

`stdb_table!` expands with zero SDK references, so the `__sdk` re-export is deleted rather than
named a seam.

**Keyless table support is deleted outright**: `TableRegistration::non_pk`, `forward_keyless`,
`KeylessMessagesMask`, the BSATN key extractor, both bare macro arms, and the `spacetimedb-lib`
dev-dependency. No production user existed. Known cost: a `#[view]` handle is shaped exactly like
a keyless table, so views are unregisterable until the keyless path returns. Re-entry trigger: the
first `#[view]` in the Module reintroduces `non_pk` (a pk registration with a narrower mask plus a
caller-supplied key fn; the BSATN extractor lives in git history).

### Connection contract

Dissolved: no new trait is invented. Identity now arrives through the lifecycle sink and the row
path reaches tables through `DbAccess`, so core's whole remaining demand on the connection is
`StdbConn` (the `'static + Send + Sync` Res-ability marker) plus `DbAccess` at table-reach sites.
A contract trait would be speculative abstraction with zero core consumers. The Game reaches
reducers via the Bindings' public `reducers` field through `StdbConnection`'s `Deref`, dropping
its `DbContext` import; reducer calls are Game-to-Bindings business, and the Bridge owns only
Reducer outcomes.

## Enforcement

**Crate split, compiler-enforced, sole mechanism.**

- `sdk_impl` moves to a new workspace crate, `stdb_bevy_sdk`: today's contents plus the
  `SdkPluginExt` and `SdkReducerSinkExt` extension traits, re-exported at its crate root per the
  `SyncAppExt` precedent. It owns the `spacetimedb-sdk` dependency, including the wasm-target
  `browser` feature.
- Core `stdb_bevy` drops `spacetimedb-sdk` from its `Cargo.toml` entirely. Dependency absence is
  the enforcement.
- The Game depends on both crates, no facade re-export: `stdb_bevy` for core, `stdb_bevy_sdk` for
  the SDK entry points, keeping the coupling point visible at the import site.
- No auxiliary tooling. An interim CI grep guard would police known-dirty code and then be deleted
  at split time. A grep guard as the mechanism was rejected for pattern upkeep and dodgeability
  (renames, re-exports); clippy `disallowed-types` was rejected because it requires enumerating
  every SDK type and misses trait bounds and free functions; cargo-deny after the split was
  rejected because a re-added dependency is a loud `Cargo.toml` diff in review, and this ADR
  states the rule.

## Supersedes

- **The original ADR 0013** ("bridge clean architecture refactor"), which lived on a branch that
  was deleted. Its strangler migration, `api/game` / `api/sdk` / `core` folder layout, and slice
  plan are abandoned; the current module layout of `stdb_bevy` is accepted as is. The crate split
  survives, recast from a migration finale into the enforcement mechanism.
- **Issue #26**, the migration mission executing that ADR. Its verdicts were re-derived from code
  rather than trusted: registration dropping its `sdk_impl` import was confirmed, while "relocate
  `RowForwarder` and the BSATN key into `sdk_impl`" was overturned (the forwarder stays in core on
  Bridge-owned traits, and the BSATN key is deleted with the keyless path) and the "minimal
  connection contract" dissolved. The old inventory's accepted couplings (Bevy itself, and
  business logic living in Bevy systems) stay accepted.

## Consequences

- The compiler owns the boundary: after the split, an SDK reference in core is a build error, and
  an SDK version migration touches only `stdb_bevy_sdk` plus the Bindings.
- The glossary's **Bridge** entry now spans both crates: `stdb_bevy` (the SDK-free core) plus
  `stdb_bevy_sdk` (the adapter binding the core's seams to the SDK).
- Implementing the fixes is a separate, later effort. Working prototypes of the identity and
  row-path shapes exist on the `sdk_coupling_fixes` branch, uncommitted.
- Owed at implementation, recorded here so they are not lost:
  - `crates/game/src/cursor.rs` drops its `spacetimedb_sdk::DbContext` import and calls
    `connection.reducers.set_cursor_position(x, y)` via field access.
  - The commented-out `TableRegistration::pk` example in `crates/game/src/main.rs` still reads
    `conn.db()` and needs refreshing.
  - `docs/spacetimedb.md` section 10 claims views "need no new Bridge support" via `non_pk` /
    `forward_keyless`; with the keyless path deleted that consequence bullet needs correcting.
  - The reducer abort-fold test moves from core to the adapter's tests.
- `StdbIdentity`'s derived `Debug` prints a raw byte array where the SDK displays hex. Accepted:
  core never logs the identity; the adapter logs the SDK-side identity at the seam.
- A recorded reservation: the developer considers the `Driver` catch-all a compromise; the growth
  policy is the agreed path to real semantic variants.
