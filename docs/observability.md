# Observability

## When to use

Read this before adding or changing any logging in the `stdb_bevy` Bridge, and when you need to
debug a misbehaving connection (a flapping client, a reconnect that never settles, a resync that
drops the wrong rows). It describes what the Bridge reports today, the conventions every log line
follows, and what is deliberately *not* reported (and why).

## Scope

Bridge observability is **dev-facing structured logging** over `tracing` (which Bevy's `LogPlugin`
already wires up on both native and wasm). The audience is a developer reading a log stream. There
is no metrics export, no on-screen overlay, and no distributed tracing. Anything that aggregates or
ships telemetry off the client is out of scope: the Bridge's headline target is wasm, where a
metrics exporter cannot run, so that work belongs server-side if it is ever wanted (see
[Deliberately excluded](#deliberately-excluded)).

## Conventions

Every Bridge log line follows these rules. New code matches them; treat a deviation as a bug.

### Structured fields, not interpolation

Carry data as `tracing` fields, with a short static message:

```rust
// good
bevy::log::info!(table = label, inserted, updated, deleted, "resync diff");
// avoid
bevy::log::info!("resync of {label}: +{inserted} ~{updated} -{deleted}");
```

Fields are filterable and machine-readable; an interpolated string is neither. Field keys are
identifiers (snake_case), so they never contain hyphens.

### The level ladder

The Bridge uses only **`trace`**, **`info`**, **`warn`**, and **`error`**:

- **`debug` is reserved for the Game.** The Bridge never logs at `debug`, so a Game can set
  `game=debug` (or filter `debug` globally) without the Bridge's internals flooding it.
- **`error` means a Bridge *bug*** (a poisoned lock, a dropped channel receiver), not an expected
  operational event. A connect that fails while the server is down, or a give-up at max retries, is
  normal operation and logs at `warn`, never `error`.
- **`info` is the milestone narrative**: connected, and the per-table resync diff. Reading at
  `info` tells you a flap happened and what it cost, without the blow-by-blow.
- **`trace` is the blow-by-blow**: individual reconnect attempts, per-row forwards, subscription
  applies. Off by default; turned on per-path when investigating.

### Module-path targets

Do not set `target =` by hand. `tracing`'s default target is the module path, so a line in
`reconnect.rs` is already targeted `stdb_bevy::lifecycle::reconnect` and a row-forward line
`stdb_bevy::row::row_channel`. This is what lets you raise one path to `trace` without raising the
others (see [Recipes](#rust_log-recipes)). Keep related logs in the module whose path you would want
to filter on.

## Catalog

| Event | Level | Fields | Origin |
| --- | --- | --- | --- |
| Connected | `info` | `identity` | SDK adapter `on_connect` |
| Resync diff (per registered table) | `info` | `table`, `inserted`, `updated`, `deleted` | resync system |
| Unintended disconnect | `warn` | `had_connection` | lifecycle drain |
| Connect failed | `warn` | `error` | lifecycle drain |
| Give-up (max retries reached) | `warn` | `retry_count` | reconnect engine |
| Subscription failed | `warn` | `entity`, `error` | subscription drain |
| Connect attempt | `trace` | `retry_count`, `delay_ms` | reconnect engine |
| Deliberate disconnect | `trace` | | lifecycle drain |
| Row insert/update/delete forwarded | `trace` | `table` | row drain |
| Subscription applied / unsubscribed | `trace` | `entity` | subscription drain |
| SDK-reported disconnect cause | `trace` | `error` | SDK adapter `on_disconnect` |
| Channel send / lock failure | `error` | `err` | channels, token |

Notes:

- The `connected` line is the only one emitted by the SDK adapter rather than the lifecycle drain,
  because the adapter is the only place that has the `identity`.
- Each lifecycle event is logged **once**, at the layer that knows enough to level it correctly. The
  drain knows intent (so it tells an unintended drop from a deliberate one) and the error cause; the
  adapter only re-states the SDK's raw disconnect cause at `trace` as drill-in detail.
- `table` is the table's accessor name (`player`, `cursor`), threaded from the `stdb_table!`
  registration, so it matches the SQL name and `db().player()`.
- A resync that reconciles nothing stays silent; only a non-empty diff logs.

## RUST_LOG recipes

`RUST_LOG` is read by Bevy's `LogPlugin` (set it in `.env`). The shipped default is:

```
RUST_LOG=warn,game=trace,stdb_bevy=info
```

That surfaces the milestone narrative: an unintended disconnect (`warn`), the eventual `connected`
(`info`), and each table's `resync diff` (`info`). Because the subscriber timestamps every line, the
gap between `disconnected` and `connected` is the outage length, read straight off the stream.

To investigate a specific flap, raise one path to `trace`:

```
# reconnect attempt-by-attempt (retry_count, backoff delay), without row noise
RUST_LOG=warn,game=trace,stdb_bevy=info,stdb_bevy::lifecycle=trace

# every forwarded row + subscription lifecycle
RUST_LOG=warn,game=trace,stdb_bevy=info,stdb_bevy::row=trace,stdb_bevy::subscription=trace
```

## Deliberately excluded

These were considered and left out on purpose. Recorded here so the next person who asks "why is
there no X?" has the answer.

- **Channel depth / backpressure.** The Bridge's channels are drained to empty every frame, so a
  backlog cannot accumulate across frames; depth would only ever measure one frame's burst. There is
  no backpressure to watch, so there is no depth metric. A drain that stops running would instead
  show up as missing row updates.
- **Latency / outage duration as a field.** Not measured. The outage length is the
  `disconnected` to `connected` gap in the line timestamps the subscriber already prints, so a
  dedicated duration field would be redundant.
- **Round-trip time (RTT).** The latency a multiplayer game cares about (input to server to echo) is
  not observable from the Bridge: the SDK exposes no RTT, and reducer events are Module-specific,
  which the module-agnostic Bridge never wraps. RTT is only measurable as a Game-level active
  reducer-ping (stamp the call, measure when its reducer event arrives), which is a separate feature,
  not Bridge logging.
- **Metrics + on-screen overlay.** Bevy ships a usable diagnostics overlay
  (`bevy_dev_tools::diagnostics_overlay`) that renders any registered `bevy_diagnostic` value. It is
  the natural upgrade if live gauges are ever wanted: the Bridge would register diagnostics at the
  same sites that log today, and a Game would render them. It was deferred because the headline want
  (RTT) is not a passive gauge, and the log stream covers the flapping-debug case without it.

## Verifying

Logging is verified by behavior and by eye, not by asserting on log strings (a brittle,
implementation-coupled test). The data behind every line is already under behavioral test: the
reconnect engine asserts `retry_count` and give-up, the resync tests assert the exact insert/update/
delete counts the `info` line reports, and the lifecycle tests assert every transition. Keep those
green (`just check::all`).

To verify the *output*, force a flap against a running native client:

1. `docker compose up` and `just dev::native` (or `just dev::clients 2`).
2. Once connected, `docker compose restart spacetimedb` (or `down` then `up`) to drop the link.
3. Watch the stream at the default filter: an `unintended disconnect` (`warn`), `connect failed`
   retries while the server is down, then `connected` and the `resync diff` once it returns.
4. Re-run with `stdb_bevy::lifecycle=trace` to see each `connecting` attempt and its backoff.
