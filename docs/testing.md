# Testing

## When to use

Read this before writing or changing any test in the workspace, and before designing code that
needs to be testable. It describes how tests are written in this repo today and how to replicate
the patterns in new code.

## Two layers, kept physically separate

The `stdb_bevy` Bridge is the reference for how the workspace is tested. Tests come in two layers:

### Unit tests — colocated

Live in the module they cover, inside a `#[cfg(test)] mod tests` block. They may poke the module's
internal seams directly (`pub(crate)` items, private helpers). Anything only reachable through a
crate-internal item is necessarily tested here.

```rust
// crates/stdb_bevy/src/utils/backoff.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_backoff_is_constant() {
        let backoff = Backoff::Fixed(Duration::from_secs(2));
        assert_eq!(backoff.delay(0), Duration::from_secs(2));
        assert_eq!(backoff.delay(5), Duration::from_secs(2));
    }
}
```

### End-to-end tests — `tests/`, public API only

Live in `crates/<crate>/tests/` and drive the crate through its **public API only** — no crate
internals. For the Bridge this means a public fake driver, public messages, and public resources;
it proves a consumer (the Game) can actually use the crate, catching "the internals work but the
crate isn't usable" regressions.

```rust
// crates/stdb_bevy/tests/connection_lifecycle.rs
use stdb_bevy::test_support::{DeferredDriver, test_app};
use stdb_bevy::{StdbConnect, StdbStatus};

#[test]
fn connect_in_flight_is_connecting() {
    let mut app = test_app(DeferredDriver::default());

    app.world_mut().trigger(StdbConnect);
    app.update();

    assert_eq!(
        *app.world().resource::<StdbStatus>(),
        StdbStatus::Connecting,
        "while a build is in flight the status is Connecting, not Disconnected",
    );
}
```

## Engine vs SDK adapter: what is testable at all

Orthogonal to the unit/e2e split above, the Bridge is layered so that almost all of it can be
tested with no server and no real socket:

- **Engine** (everything outside `src/sdk_impl/`): the channels and their drain systems, the
  lifecycle wiring, reconnect and backoff, the row mirror, and resync. It is generic over the
  connection type (`StdbConn`, which asks only for `Send + Sync + 'static`) and over the driver
  traits (`StdbConnectionDriver`, `StdbSubscriptionDriver`), so a test instantiates it with
  `FakeConn` plus a fake driver. This layer is where behavior lives, and all of it is covered.
- **SDK adapter** (`src/sdk_impl/`): `SdkConnectionDriver` and `SdkSubscriptionDriver`. Builds the
  real `DbConnection` (native blocking vs wasm `spawn_local`), pumps `frame_tick` each frame, and
  installs the real SDK callbacks (`on_disconnect`, `on_applied`, and via `RowForwarder` the
  per-table `on_insert`/`on_delete`). Needs a live server, so it is verified by hand against
  `just dev`, not in CI.

The seam between them is the set of sinks (`LifecycleSink`, `RowForwarder`, `SubscriptionSink`,
`ReducerOutcomeSink`). Production and tests push through the *same* sink, so a test drives the real
drain systems and asserts on the real messages:

```
production:  frame_tick() -> SDK on_insert(|_, row| sink.insert(row.clone())) -> channel -> drain -> RowInserted<T>
test:                                     sink.insert(row)                    -> channel -> drain -> RowInserted<T>
```

The only thing left uncovered is the one-line callback body itself.

**Consequence for new code:** keep behavior in the engine layer and let the adapter stay a thin
translation with nothing worth asserting. Wanting to write a test against something in `sdk_impl`
is the signal that the logic belongs one layer down.

## Reusable fakes behind a `test-support` feature

Shared test doubles (`FakeConn`, `FakeConnectionDriver`, `DeferredDriver`, `FakeTable`, `test_app`)
live in a `test_support` module exposed behind an off-by-default `test-support` feature, so they are
available to both the crate's own tests and downstream crates (the Game reuses them), but never ship
in a normal build:

```rust
// in the crate root (e.g. lib.rs)
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
```

`cargo test` turns the feature on for the integration-test build via a **self-dev-dependency** —
the crate depends on itself with the feature enabled, the standard way to make a feature available
to a crate's own `tests/`:

```toml
[dev-dependencies]
stdb_bevy = { path = ".", features = ["test-support"] }
```

When a fake needs to construct a non-public type (e.g. a connection error), it exposes a small
intent-revealing method (`DeferredDriver::deliver_error`) rather than leaking the type, so the
e2e tests stay public-API-only.

## Conventions

- **Start every colocated `mod tests` with `use super::*;`** so the module's items are in scope
  without repeating imports in each test.
- **Name tests after the behavior**, in `snake_case`, as a sentence:
  `connecting_resolves_to_connected_when_the_build_lands`, not `test_connect`. The name reads like a
  spec line.
- **One behavior per test.** Don't cram multiple behaviors into one test; split them so a failure
  points at exactly one thing.
- **Cover happy and unhappy paths.** For each piece of behavior, test the success path *and* the
  failure/edge paths (errors, caps, overflow, empty input). Existing tests pair, e.g.,
  `connecting_resolves_to_connected_when_the_build_lands` with
  `connecting_resolves_to_disconnected_on_error_and_rearms_reconnect`.
- **Assertion messages carry the *why*.** When an assertion's intent isn't obvious from the values,
  add a message explaining the expected behavior — it documents intent and makes failures
  self-explaining:

  ```rust
  assert_eq!(
      probe.connects(),
      1,
      "Connecting must suppress auto-reconnect — no second build may be kicked in flight",
  );
  ```

- **Test behavior, not implementation.** Assert on observable results through the interface, not on
  internal call counts or private state. A test should survive an internal refactor that preserves
  behavior.
- **Design for testability.** Accept dependencies instead of constructing them internally (so a fake
  can be injected), and prefer returning results over hidden side effects.

## Running tests

- `just check::test` — runs the workspace test suite.
- `cargo test --workspace --exclude stdb_module` — the underlying command. The wasm-only
  `stdb_module` is **excluded** from host cargo commands (it only compiles for
  `wasm32-unknown-unknown`); confirm it builds with `just check::module`.
- `just check::all` — fmt + clippy + test + the Module wasm build.

## References

- CI runs the same gates — see `docs/cicd.md`.
- Domain vocabulary for test and interface names lives in the project glossary.
