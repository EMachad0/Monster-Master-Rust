# Explicit connection with a desired-intent model

The `stdb_bevy` Bridge does **not** connect on its own. Adding `StdbPlugin` to an `App` installs
the engine but establishes no socket; the Game triggers the connection explicitly (e.g. after a menu
click, or once the player has logged in). Connection is modelled as **desired intent vs. actual
status**, and auto-reconnect reconciles the two.

## Considered Options

- **Auto-connect on plugin build (what `bevy_spacetimedb` does).** Rejected: many games must *gate*
  the connection — behind a main menu, a "Play" button, or an email/password login — so connecting the
  instant the plugin is added is wrong for them, and it couples `App` construction to network I/O.
- **Explicit trigger, but status-only (no intent).** Rejected: with only `StdbStatus`, the states
  "never connected", "user explicitly disconnected", and "dropped unexpectedly" are indistinguishable
  (all `Disconnected`). Auto-reconnect would then either fire at startup (defeating explicit connect)
  or never fire after a real drop.
- **Explicit trigger + a desired-intent model (chosen).** `StdbConnect` / `StdbDisconnect` observer
  events set a `StdbIntent` resource (`Connected` / `Disconnected`, default `Disconnected`).
  `StdbStatus` reports only the *actual* connection (`Disconnected` / `Connecting` / `Connected`).
  Auto-reconnect fires only when `intent == Connected && status == Disconnected`.

## Consequences

- The first connection is explicit. The `StdbConnect` observer sets `intent = Connected`, status
  `Connecting`, and kicks the build **immediately** (no backoff). Auto-reconnect after a drop goes
  through the **same `kick_build` helper** but gated by the backoff/jitter/max-retries policy. Initial
  connect = immediate; reconnects = backed off.
- `StdbStatus` starts `Disconnected`; `Connecting` means a build is in flight; "never connected" is
  simply `status == Disconnected && intent == Disconnected`. No `Idle` variant needed.
- `StdbDisconnect` reuses the unexpected-drop path entirely (remove `StdbConnection`, run the
  delete-sweep, fire `StdbDisconnected`); the *only* difference is it sets `intent = Disconnected`, so
  reconnect stays quiet. A disconnect requested mid-`Connecting` is reconciled when the build lands (the
  `Connected` handler disconnects immediately if `intent == Disconnected`).
- `StdbPlugin::connect_on_startup()` is **sugar** that registers a Startup system triggering
  `StdbConnect`; it is not a special path — the Bridge still "never connects unless triggered."
- The request observers (`StdbConnect` / `StdbDisconnect`, imperative) are deliberately distinct from
  the result observers (`StdbConnected` / `StdbDisconnected`, past tense).
- Costs more API surface than auto-connect (`StdbConnect`/`StdbDisconnect`/`StdbIntent`), but cleanly
  supports menu-gated and auth-gated connection, and makes auto-reconnect a trivial, testable predicate
  over (intent, status).
- Intent is *fulfilled* by reconciliation through a **`StdbConnectionDriver`** trait the Bridge
  defines (`connect` / `tick` / `disconnect`) — the SDK has no unifying trait (`build` is on
  `DbConnectionBuilder`, `frame_tick` is an inherent method, `disconnect` is on `DbContext`), and the
  driver *drives* the whole connection lifecycle (hence the name, not "Connector"). A test
  `FakeConnector` makes the connect/disconnect/reconnect reconciliation unit-testable with no socket.
- The Bridge ships **`SdkConnectionDriver<M: SpacetimeModule>`** as the default driver. `SpacetimeModule`
  is exposed *only* under `spacetimedb_sdk::__codegen` (doc-hidden: "may change without a major version")
  — but a bridge-provided, generic-over-module driver unavoidably needs it, since
  `DbConnectionBuilder::<M>::new()` requires `M: SpacetimeModule`. Verified: the SDK has no public alias,
  and both the generated bindings and `bevy_spacetimedb` rely on it (the reference bounds on
  `__codegen::SpacetimeModule` *and* `__codegen::DbConnection`; we need only the former). This
  `__codegen` touch is **contained to the `SdkConnectionDriver` adapter file** — the trait, engine,
  lifecycle, rows, and reconnect stay `__codegen`-free. A Game that wants to avoid `__codegen` entirely
  can write its own `StdbConnectionDriver` against the *generated* public `DbConnection::builder()`
  (the trait and `LifecycleSink` are public for exactly that escape hatch).
