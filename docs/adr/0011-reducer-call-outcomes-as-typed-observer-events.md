---
status: accepted (extends ADR 0002)
---

# Reducer-call outcomes as typed observer events; other clients' reducer events are out of reach

The Bridge modeled the server-to-client direction fully (rows arrive as
`RowInserted`/`RowUpdated`/`RowDeleted`), but the client-to-server return path was unmodeled: a Game
fired `connection.reducers().<reducer>()` and the `Result` told it only whether the call was *sent*,
never whether the reducer *committed*. This ADR surfaces the caller's own reducer outcome to the Game
as typed observer events, `ReducerCommitted<K>` and `ReducerFailed<K>`, keyed by a Game-chosen marker
type `K`, through the same channel-then-drain seam the rows use. ADR 0002 flagged reducer-outcome
events as a near-term follow-up; this is it.

## What the SDK 2.6 actually delivers

Verified against `spacetimedb-sdk` 2.6 source, because it bounds the whole design:

- There is no persistent `on_reducer`. The only reducer callback is the per-call one-shot
  `<reducer>_then`, stored by `request_id` and popped when the matching result arrives.
- A client is notified only of reducers it invoked itself. Every other client's transaction arrives
  as `Event::Transaction`, a dataless variant carrying no reducer name, arguments, caller identity,
  or status.
- `ReducerEvent` carries only `timestamp`, `status` (`Committed | Err(String) | Panic(InternalError)`),
  and `reducer` (the args enum). There is no caller identity (for your own call it is your own), no
  energy, and no request id exposed.
- `set_reducer_flags` does not exist; `CallReducerFlags::Default` is hardcoded.

## Considered Options

### Which capability is in scope

- **Everyone's reducer events (not a choice, a limit).** The plan that motivated this work asked to
  observe all players' reducer events (an emote, a spell cast, a chat message not persisted as a row)
  via a supposed `on_reducer`. SDK 2.6 cannot deliver it: no `on_reducer`, and other clients' runs
  surface only as dataless `Event::Transaction`. Cross-player, event-shaped feedback must be modeled
  as a Row (even an ephemeral one) that subscribers see, which already flows through the existing row
  path. Recorded explicitly so it is not re-attempted.
- **The caller's own call outcome (chosen).** This the SDK does deliver, through the `<reducer>_then`
  callback. It is the primitive gameplay needs: react when the server rejects your attack, buy, or
  join.

### Message shape and per-reducer reactivity

A hard requirement was that a Game system react to one specific reducer's outcome. Bevy dispatches
observers and message readers by type, so this forces a per-reducer type. The bindings expose no
public per-reducer value type (the args structs are `pub(super)`, and the generated per-reducer trait
is not object-safe because `_then` takes `impl FnOnce`, so it cannot serve as a `dyn` tag). The Game
therefore supplies a zero-field marker type `K` (`struct Attack;`), the reducer counterpart of the row
marker in `StdbSync`. Two events (`ReducerCommitted<K>`, `ReducerFailed<K>`) rather than one status
enum, so an observer can watch only failures without matching.

### Observer versus buffered message

Buffered messages were rejected: `Messages<T>` must be registered with `add_message` per type, which
reintroduces a per-reducer registration list. Observer `Event` types self-register via `add_observer`,
so outcomes are delivered as observer events and the Game opts in with an observer plus the typed call,
registering nothing on the plugin.

### One erased channel versus a typed channel per reducer

The row path uses a typed `RowChannel<R>` plus a `drain_row_sink::<R>` system registered per table in
`add_tables`. Mirroring that for reducers would register a `drain::<K>` per reducer, the registration
we ruled out. Instead a single channel carries a type-erased `Box<dyn Command>` and one drain applies
them, so no per-reducer system is registered. The `_then` callback runs inside `frame_tick` with no
`World` access (the same reason rows use a channel), so the per-`K` trigger must be deferred through
the channel; a single non-generic drain cannot name every `K`, so the per-`K` action rides through
erased as a Command. The cost is an erased payload; the benefit is one drain and zero registration.

### In-flight calls across a disconnect

The SDK never fires the pending `reducer_callbacks` on disconnect, and the Bridge cannot reach that
map to synthesize a failure. A call in flight when the socket drops therefore produces no outcome.
Accepted as a known gap, the reducer analog of the row ghost gap in ADR 0002, to be fixed when felt.
Because an outcome is a point-in-time event and not state to reconcile, the drain runs every frame
ungated, with no resync and no sink clearing.

## Consequences

- New public surface: `ReducerCommitted<K>` and `ReducerFailed<K>` observer events, and a
  `ReducerOutcomeSink` resource whose `cb::<K, _>()` adapts a Game marker into the SDK `_then`
  callback. The channel and drain are wired once by the plugin.
- Per reducer the Game writes one marker (`struct Attack;`) and appends `sink.cb::<Attack, _>()` to its
  existing `connection.reducers().attack_then(..)` call, then reacts with `On<ReducerFailed<Attack>>`.
  No plugin registration.
- The Bridge stays module-agnostic: it never names the module `Reducer` enum, `RemoteReducers`, or
  `ReducerEventContext`. The marker and the closure live Game-side, and the callback's context type is
  a generic the compiler infers, which also makes the seam unit-testable with a unit context.
- `ReducerCommitted<K>` carries no payload (the Game already holds the arguments it sent);
  `ReducerFailed<K>` carries the error string; a `Panic(InternalError)` folds into `ReducerFailed`.
- Known gap: an in-flight call across a disconnect is dropped silently.
- Open wart: `cb::<K, _>()` needs the trailing `_` for the inferred context type; no way to drop it
  was found without giving up the per-reducer type.
