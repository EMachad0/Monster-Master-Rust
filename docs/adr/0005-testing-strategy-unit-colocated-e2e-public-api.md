# Testing strategy: unit tests colocated, e2e through the public API via a `test-support` feature

The `stdb_bevy` Bridge is tested at two layers, kept physically separate:

- **Unit tests** live colocated in each module's `#[cfg(test)] mod tests` and may poke the module's
  internal seams directly (`LifecycleChannel`/`RowChannel` sinks, `ReconnectState::tick`,
  `RowForwarder` + a `FakeTable`, the lifecycle drain). Anything only reachable through a
  `pub(crate)` item (e.g. `StdbIntent`) is necessarily tested here.
- **End-to-end tests** live in `crates/stdb_bevy/tests/` and drive a whole `StdbPlugin` through the
  connect lifecycle using **only the public API** — a `StdbConnectionDriver` fake, `StdbConnect`,
  and the public `StdbStatus`/row messages. No crate internals.

The reusable fakes (`FakeConn`, `FakeConnectionDriver`, `DeferredDriver`, `FakeTable`, `test_app`)
live in `test_support`, exposed behind an off-by-default **`test-support` feature**
(`#[cfg(any(test, feature = "test-support"))] pub mod test_support`). `cargo test` turns the feature
on for the integration-test build via a **self-dev-dependency**
(`[dev-dependencies] stdb_bevy = { path = ".", features = ["test-support"] }`).

## Considered Options

- **All tests in-crate (`#[cfg(test)]`), including a `mod e2e`.** Simpler — e2e shares
  `crate::test_support` directly with no feature and no visibility concerns. Rejected as the *e2e*
  home: it never exercises the public boundary, so "the internals work but the crate isn't usable"
  regressions slip through.
- **e2e in `tests/`, public API only, with a `test-support` feature (chosen).** For a *bridge* crate
  whose whole purpose is to be a usable public surface, forcing the e2e suite through that surface is
  the most valuable kind of test — it proves a Game can actually build a driver, declare a table, and
  read messages. The `test-support` feature is a modest, genuinely reusable cost (the Game can use
  the same fakes in its own tests).

## Consequences

- The e2e suite can only touch public items, which keeps the public API honest (the fakes are built
  from pub `StdbConnectionDriver` + `LifecycleSink`; results are observed via pub `StdbStatus` and
  `RowInserted<T>`).
- The **self-dev-dependency** is the standard way to enable a feature for a crate's own integration
  tests; without it `cargo test` wouldn't compile `tests/` with `test_support` available. A future
  reader puzzled by "why does the crate depend on itself" should look here.
- `test_support` is feature-gated, so it never ships in a normal (`--release`, no-feature) build.
- Test fakes that need to hide a non-public type (e.g. constructing a `ConnectionError`) expose a
  small intent-revealing method instead (`DeferredDriver::deliver_error`), so the e2e tests stay
  public-API-only.
