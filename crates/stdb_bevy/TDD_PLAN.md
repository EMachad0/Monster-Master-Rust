# stdb_bevy — TDD slice plan

Implementation of the Bridge per ADR 0002 and `docs/spacetimedb.md` rule #4.

## How we work

- **You write the code, I write the tests.** Strict vertical TDD: per slice, I write **one** failing
  test (RED), you write the minimal code to pass (GREEN), then we move on. No batching tests ahead of
  implementation.
- Each slice lists **Behavior** (the spec the test encodes), **You implement** (the public interface),
  and **I test** (the seam + assertion). Interfaces are *proposed* — adjust them as the implementation
  teaches us; the test follows the real interface.
- Definition of done per slice: `cargo test -p stdb_bevy` green, then `just check::all` stays green.

## Testability strategy

The Bridge splits into two layers so the first is unit-testable with no server:

1. **Engine (TDD'd here):** message types, the per-table ingest channel + drain systems, the
   lifecycle → observers/`StdbStatus`/`StdbConnection` wiring, the `stdb_connected` run condition, the
   disconnect delete-sweep, and the reconnect-backoff / token-reuse *logic*. The engine is **generic
   over the connection type `C: Send + Sync + 'static`**, so tests instantiate it with a trivial
   **fake `C`** — no real `DbConnection` needed.
2. **SDK adapter (manual/integration):** building the real connection (native-sync vs
   wasm-`spawn_local`), calling `frame_tick`, installing real SDK `on_insert`/`on_disconnect`/… that
   push into the engine's channels, and the actual reconnect socket work. Verified by hand against
   `just dev`, not in CI.

### The test seam, concretely

The Bridge's real job is "something happens on the connection → it shows up in Bevy." In production a
**`frame_tick`-driven SDK callback** is what makes the "something happen." A test can't call that
callback (it needs an `EventContext` we can't fabricate). So we put a **channel** one level below the
callback, and **both production and tests push through it**:

```
production:  frame_tick() → SDK on_insert(|_, row| sink.insert(row.clone())) → channel → drain system → RowInserted<T> message
test:                                              sink.insert(foo)          → channel → drain system → RowInserted<T> message
```

The drain system and the message — the behavior we care about — are identical on both paths. The only
thing the unit test doesn't cover is the trivial one-line callback `|_, row| sink.insert(row.clone())`,
which the manual layer verifies. The channel isn't an incidental internal detail; it *is* the Bridge's
ingest mechanism, so testing through it is testing real behavior.

Sketch (proposed — names/shape are yours to set):

```rust
// You implement (engine):
#[derive(Message)] pub struct RowInserted<T>(pub T);
enum RowEvent<T> { Insert(T), Update(T, T), Delete(T) }
#[derive(Clone)] pub struct RowSink<T>(crossbeam_channel::Sender<RowEvent<T>>);
impl<T> RowSink<T> { pub fn insert(&self, row: T) { let _ = self.0.send(RowEvent::Insert(row)); } /*…*/ }
#[derive(Resource)] pub struct RowChannel<T> { pub sink: RowSink<T>, rx: crossbeam_channel::Receiver<RowEvent<T>> }

pub fn register_table_events<T: Clone + Send + Sync + 'static>(app: &mut App) {
    app.add_message::<RowInserted<T>>();
    let (tx, rx) = crossbeam_channel::unbounded();
    app.insert_resource(RowChannel { sink: RowSink(tx), rx });
    app.add_systems(Update, drain_rows::<T>);
}
fn drain_rows<T: Send + Sync + 'static>(chan: Res<RowChannel<T>>, mut w: MessageWriter<RowInserted<T>>) {
    while let Ok(ev) = chan.rx.try_recv() { if let RowEvent::Insert(r) = ev { w.write(RowInserted(r)); } }
}

// I test:
let mut app = App::new();
app.add_plugins(MinimalPlugins);
register_table_events::<Foo>(&mut app);
let sink = app.world().resource::<RowChannel<Foo>>().sink.clone();  // same sink production would use
sink.insert(Foo { id: 7 });
app.update();
// assert exactly one RowInserted<Foo>(Foo { id: 7 })
```

My two earlier questions, restated plainly:
1. **Where does the test get the sink?** Above I *store it in a resource* (`RowChannel<T>.sink`), so both
   the test and the SDK adapter fetch it the same way. The alternative was *returning* the sink from
   `register_table_events`. Storing-in-resource works for both callers → I'll go with that unless you
   prefer returning it.
2. **Channel crate** → `crossbeam-channel` (its `Receiver` is `Send + Sync`, so `RowChannel` is a normal
   `Res`, no `NonSend`). You're fine with it. ✅

**Lifecycle uses the exact same seam:** a `LifecycleSink` carrying `Connected(C) / Disconnected /
Error(e)`; the test pushes `Connected(fake_c)`, a drain applies it (sets `StdbStatus`, inserts/removes
`StdbConnection<C>`, triggers the observer). Because the engine is generic over `C`, the test's `C` is a
trivial fake struct.

The full `StdbPlugin` installs this engine **and** a startup adapter that drives the real connection.
Tests install just the engine (a generic `fn`/sub-plugin) with a fake `C`, never starting a socket.

---

## Phase A — Connection lifecycle (generic over a fake `C`)

### Slice 1 (tracer bullet) — connected → observer + status + resource
- **Behavior:** a "connected" lifecycle signal triggers the `StdbConnected` observer, sets
  `StdbStatus::Connected`, and inserts `StdbConnection<C>`.
- **You implement:** `StdbConnected` (observer `Event`), `enum StdbStatus { Connecting, Connected,
  Disconnected }` (`Res`), `StdbConnection<C>`, the `LifecycleSink` seam, and the drain that applies it
  — installed by an engine `fn`/sub-plugin generic over `C`.
- **I test:** with a fake `C`, push `Connected(fake)`; `update()`; assert an observer flag was set,
  `StdbStatus::Connected`, and `StdbConnection<Fake>` present.

### Slice 2 — disconnected → observer + status + resource removed
- **Behavior:** a "disconnected" signal triggers `StdbDisconnected`, sets `StdbStatus::Disconnected`,
  removes `StdbConnection<C>`.
- **I test:** connect (Slice 1) then push `Disconnected`; assert observer flag, status, resource gone.

### Slice 3 — connect error → observer + status
- **Behavior:** a "connect error" signal triggers `StdbConnectionError` (carrying the error) and leaves
  `StdbStatus::Disconnected`.
- **I test:** push an error signal; assert observer fired with the message and status `Disconnected`.

### Slice 4 — `stdb_connected` run condition gates systems
- **Behavior:** a system gated `.run_if(stdb_connected::<C>)` runs only while connected.
- **I test:** counter system gated by the condition; 0 before connect, increments after `Connected`,
  stops after `Disconnected`.

---

## Phase B — Row-change messages

### Slice 5 — insert → `RowInserted<T>`
- **Behavior:** a row pushed as an insert becomes a `RowInserted<T>` message next `update()`.
- **You implement:** `RowInserted<T>(pub T)`, the `RowChannel<T>`/`RowSink<T>` seam, `drain_rows::<T>`,
  and `register_table_events::<T>(app)` (see sketch above).
- **I test:** register for `Foo`; `sink.insert(Foo{..})`; `update()`; assert one `RowInserted<Foo>`.

### Slice 6 — update → `RowUpdated<T> { old, new }`
- **You implement:** `RowUpdated<T> { pub old: T, pub new: T }` + its drain arm.
- **I test:** `sink.update(old, new)`; assert one `RowUpdated<Foo>` with both values.

### Slice 7 — delete → `RowDeleted<T>`
- **You implement:** `RowDeleted<T>(pub T)` + its drain arm.
- **I test:** `sink.delete(Foo{..})`; assert one `RowDeleted<Foo>`.

### Slice 8 — bulk burst preserves count & order
- **Behavior:** N inserts queued in one frame produce N `RowInserted<Foo>` in send order (initial
  subscription burst).
- **I test:** push 3 inserts before one `update()`; assert 3 messages, in order.

---

## Phase C — The `stdb_tables!` macro

### Slice 9 — `stdb_tables! { foo => Foo }` wires insert/update/delete
- **Behavior:** registering via the macro yields the Phase-B behavior for `Foo`.
- **You implement:** `stdb_tables! { foo => Foo }` expanding to the registration (transient-borrow, no
  leak — accessor `db.foo()` used only inside the registration closure).
- **I test:** set up via the macro, drive the sink, assert insert/update/delete messages appear.

### Slice 10 — callback selection `foo => Foo { insert, delete }`
- **Behavior:** an explicit subset registers only those message types (also the no-PK form, since the
  SDK only gives `on_update` to primary-key tables).
- **I test:** register `{ insert, delete }`; assert `Messages<RowInserted<Foo>>` and
  `Messages<RowDeleted<Foo>>` exist but `Messages<RowUpdated<Foo>>` does **not**.

---

## Phase D — Disconnect delete-sweep — **CANCELLED**

Dropped after verifying SDK 2.4 behavior (see ADR 0002, updated): on reconnect the fresh-cache
subscription **re-fires `on_insert` for every still-matching row**, and a blanket delete-sweep would
make persisting rows despawn-then-respawn (flicker) while baking one reconciliation policy into a
module-agnostic forwarder. The Bridge now forwards **only real SDK row events**; reconciliation
across reconnect is the Game's policy (key entities by pk, treat `RowInserted` as an upsert; opt into
a clean-slate sweep via a one-line `StdbDisconnected` observer if desired).

---

## Phase E — Reconnect & token logic (pure logic, fakes)

### Slice 12 — backoff schedule
- **Behavior:** the reconnect delay grows with attempt count up to a cap, and resets after a successful
  connect.
- **You implement:** a pure `fn backoff(attempt: u32) -> Duration` (or small state struct).
- **I test:** assert the sequence of delays and the reset-on-connect.

### Slice 13 — token capture & reuse
- **Behavior:** the token from a `Connected` signal is stored and handed to the next connection build;
  dropped (back to anonymous) only on explicit reset, not across a reconnect.
- **You implement:** token state + the "next build uses stored token" decision, behind a fake build fn.
- **I test:** simulate Connected(token) → Disconnected → reconnect; assert the fake build received the
  stored token.

---

## Manual / integration (no CI; run against `just dev`)

Not TDD'd — verified by hand once the engine is green:

- [ ] SDK adapter: real `DbConnection` build with native-sync / wasm-`spawn_local(build().await)` split.
- [ ] `frame_tick` pumped once per frame; messages/observers fire end-to-end.
- [ ] Real `on_insert`/`on_update`/`on_delete`/`on_disconnect` push into the engine channels.
- [ ] Auto-reconnect after a real socket drop; identity stable via in-memory token reuse.
- [ ] `game` example compiles to native + wasm and shows online player count via the new API.
- [ ] `Send + Sync` compile assertions for `StdbConnection<DbConnection>` on both targets.

---

## Open interface questions to settle as we hit them
- Exact sink/channel type names (`RowChannel`/`RowSink`/`LifecycleSink`) and whether sinks are returned
  vs. stored in resources (currently: stored).
- Whether `StdbStatus` gains an `Error` variant or connect-errors map to `Disconnected` (currently the
  latter).
- System ordering: a `StdbSet` so drains run before Game systems read messages within the same frame.
