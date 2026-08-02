---
status: accepted (supersedes the "reconciliation across reconnect is the Game's policy" stance of ADR 0002)
---

# Reconnect Resync via a retained connection diff

The SpacetimeDB SDK has no resume: a reconnect builds a fresh `DbConnection` with an **empty Client
cache**, the re-applied Subscription re-delivers the whole **Snapshot** as inserts, and rows deleted
during the outage are never signalled — they linger as **Ghost rows** (see `../CONTEXT.md`). ADR 0002
left fixing this to each Game ("known gap, fix when felt"). We now fix it **once, in the Bridge**, so
a reconnect is transparent to the Game: it keeps its existing `RowInserted` / `RowUpdated` /
`RowDeleted<T>` handlers and sees a correct diff, with no per-table bookkeeping.

Verified SDK facts (spacetimedb-sdk 2.4.0) that force the shape below:

- **No resume.** `disconnect()` ends a connection permanently; reconnect = rebuild ⇒ fresh empty
  cache (`SdkConnectionDriver::connect` already rebuilds). The SDK's `on_disconnect` is a **no-op**
  (`subscription.rs:49`): it never clears the cache nor fires deletes, so the old cache is fully
  readable until its `DbConnection` is dropped.
- **Re-subscribe is inserts-only.** `SubscribeApplied` applies the server's `TableUpdate` to the
  fresh cache (`db_connection.rs` `apply_update`); the initial Snapshot carries only inserts, and the
  cache populates **before** `on_applied`, which fires **before** the per-row `on_insert`s — all in one
  `frame_tick`. There is no snapshot-vs-cache delete synthesis.
- **No generic `row → primary key`.** `TableWithPrimaryKey` (`table.rs:63`) exposes only `on_update`;
  the PK extractor is a generated internal closure, and the generated row type does not mark its PK
  field. So across a reconnect (where the SDK fires no `on_update`) the Bridge cannot derive updates
  without being told the key.
- **Keyless row identity is BSATN.** The cache keys keyless rows by their `bsatn` bytes, refcounted
  (`client_cache.rs` `handle_insert`/`handle_delete`). The generated row derives sats `Serialize`, so
  the Bridge can reproduce that identity by serialising.

## Considered Options

### How to recover the pre-outage state to diff against

- **Continuous row mirror.** Maintain a per-table `HashMap<Pk, Row>` of everything emitted, always.
  Rejected: it duplicates every subscribed row in memory **permanently** (~2× the cache, e.g. ~+105 MB
  at 1M ~80 B rows) to serve an event that only happens on a disconnect.
- **Snapshot-copy on disconnect.** Clone each table's rows into a `ResyncSnapshot<T>` resource at
  disconnect. Transient cost (outage-only), self-contained — but pays a full clone-all of the cache on
  every drop.
- **Retained connection (chosen).** On disconnect, **move** the live connection into a
  `PreviousConnection<C>` resource instead of dropping it; the reconnect fills a fresh
  `StdbConnection<C>`; the fence diffs `PreviousConnection` against `StdbConnection`. Steady-state cost
  is **1×** (nothing maintained); the 2× peak is **one frame** (snapshot-applied → fence), then the old
  connection is dropped. No eager clone — the diff iterates the SDK caches as-is. The old connection is
  never `frame_tick`ed (the tick system reads `StdbConnection`, not `PreviousConnection`), so it sits
  inert; its socket already closed on the drop.

### When to reconcile — the fence

- **On `StdbConnected`.** Rejected: it fires *before* the Snapshot arrives (the Subscription is issued
  on connect; the server replies a later tick), so the world is empty there — sweeping then deletes
  everything.
- **On Subscription-applied, after a reconnect (chosen).** The diff runs when every active
  Subscription has re-applied — the `is_subscriptions_settled` run-condition from ADR 0006, exactly the
  fence that ADR built. With multi-set, sweeping after only the first `on_applied` would false-delete
  rows from a not-yet-applied set, so "all settled" is load-bearing.

### How the reconnect Snapshot reaches the Game

- **Let the SDK's re-fired inserts flow through.** Rejected: every surviving row would arrive as
  `RowInserted`, re-triggering insert side effects (spawns, sounds, "joined" notifications) for rows
  that merely persisted, and a row *changed* during the outage would arrive as insert, not update.
- **Suppress during the resync window, reconstruct at the fence (chosen).** While resync is in flight
  the Bridge **discards** forwarded row messages; at the fence it emits the computed diff of
  `PreviousConnection` vs `StdbConnection`: pk new → `RowInserted`, pk gone → `RowDeleted` (full body,
  from the retained cache), pk changed → `RowUpdated{old, new}`, pk unchanged → nothing. The Game's
  existing handlers receive a faithful, minimal diff; survivors never re-fire inserts; `RowDeleted<T>`
  keeps its full row body (no API change).

### How to correlate rows: declared key vs SDK vs BSATN

- **Declared `key`, present ⟺ the PK form (chosen for PK tables).** The SDK gives no generic
  `row → pk`, so the key must be named: `stdb_table!(player => Player, key = identity)`. The presence
  of `key =` *is* the axis — it selects the PK registration (`TableRegistration::pk`), whose handle is
  bounded `T: TableWithPrimaryKey`, so it compiles only for tables that *have* a PK and yields faithful
  insert/update/delete. There is no "PK table with an omitted key" footgun: a PK table declared without
  `key` is simply the keyless form, which reconciles by BSATN instead. This key exists only to classify
  a row within the diff — same key in both snapshots is an update, present on one side only is a
  delete or insert, so it must be unique and stable per row. It is not the key a Row mirror uses to
  locate the entities backing a row (ADR 0009); the two serve unrelated purposes and need not be the
  same field.
- **BSATN byte-set diff for keyless tables (chosen for the keyless form).** A table declared *without*
  `key` — the bare `stdb_table!(monster => Monster)` — selects `TableRegistration::non_pk`, whose
  identity is the whole row. The Bridge derives that identity itself: `non_pk` bounds
  `R: sats::Serialize` and bakes a BSATN key extractor internally, so the caller never supplies a key
  and a wrong one is unrepresentable. Diffing by those bytes reproduces the SDK's own
  refcount-by-bsatn identity: ghosts = old ∉ new → delete, new ∉ old → insert, **no updates** (a change
  is delete+insert, since differing bytes are a different identity). The `__codegen` BSATN touch stays
  contained to that one extractor (per ADR 0003's contained-`__codegen` precedent).
- **Reuse the SDK's `TableUpdate`/`with_updates_by_pk` machinery.** Rejected: it is `__codegen`-internal
  and operates on *one* cache's server deltas, not a diff of two arbitrary caches; there is no public
  "diff two row sets" helper to reuse.

### Which row events a table surfaces — selection

Both forms take an optional event selection, orthogonal to the pk/keyless axis: `RowMessagesMask` for
PK (`insert`/`update`/`delete`), `KeylessMessagesMask` for keyless (`insert`/`delete` — no `update`
field). The selection is the **single source** for which events a table surfaces: it gates the live
forward callbacks and the resync diff's branches together, so the live path and the reconnect diff
can never disagree about which events a Game sees. The message types stay registered regardless, so a
deselected event simply never fires — a Game that still reads it gets nothing, not a panic.

- **Typed per-form, passed to the constructor (chosen).** `pk(.., RowMessagesMask)` and
  `non_pk(.., KeylessMessagesMask)` each name their own type, yet both still erase to
  `TableRegistration<C>`, so `add_tables`'s homogeneous array (the backward `C`-inference the macro
  relies on) is untouched. A shared `.emit()` method was rejected: one type carries one signature, so
  it could not be `RowMessagesMask` here and `KeylessMessagesMask` there, and nothing would stop
  passing the wrong one. Distinct constructor
  arguments make a cross-typed selection a plain compile error. To keep the selection unforgeable, the
  forwarder's per-event wiring is private behind `forward` / `forward_keyless`, which read the same
  baked selection — a caller cannot wire live callbacks that disagree with the diff. Defaults: PK = all
  three; keyless = insert+delete.
- **`[update]` on a keyless table is a compile error (chosen).** The macro builds the selection as a
  struct literal, so `stdb_table!(m => M, [update])` expands to `KeylessMessagesMask { update: true, .. }` — a
  non-existent field. The error is at the API type, not a trait bound, so it holds even against a test
  double that happens to impl `TableWithPrimaryKey`. (`forward_keyless` is additionally bounded
  `T: Table`, with no `on_update` to call — defence in depth.)
- **Every registration reconciles (chosen).** There is deliberately no forward-only registration: a
  table registered without a diff would never reconcile (ghosts on reconnect), defeating this ADR.
  Every registration is a `pk` or a `non_pk`, both of which diff — so the type system enforces that
  every registered table resyncs.

### Give-up / explicit disconnect: keep-everything vs clean-slate

The baseline (`PreviousConnection`) and the Game's view must share a lifetime — dropping one while
keeping the other is inconsistent (a kept view becomes un-reconcilable; a kept baseline against a
cleared view replays "no change" into an empty world).

- **Drop the baseline but keep the view.** Rejected: the inconsistent case above — a later reconnect
  cannot heal the frozen world's ghosts.
- **Keep-everything until a real reconcile (chosen default).** `PreviousConnection` is consumed in
  exactly one place — a successful reconnect's fence diff. On **give-up** (max retries, intent still
  `Connected`) and on **explicit `StdbDisconnect`**, the Bridge does nothing: it keeps the frozen view
  and the baseline, so a resumed/later connect reconciles. Cost: the baseline holds ~1× the last cache
  while disconnected (bounded, not growing).
- **Clean-slate (opt-in).** A Game that prefers a cleared world on disconnect despawns its entities in
  its `StdbDisconnected` observer **and** resets the Bridge baseline (one explicit signal), so the next
  connect is treated as fresh and re-inserts everything. The forbidden middle — clear one, keep the
  other — is what the single reset signal exists to prevent.

## Consequences

- **Transparent reconnect.** The Game keeps its `RowInserted`/`RowUpdated`/`RowDeleted<T>` handlers
  unchanged, with full row bodies; reconnect surfaces as a correct, minimal diff. No `Stale` component,
  no `TablePk`, no PK→Entity index, no per-table reconciliation code — the Bridge owns it all. This is
  the "solved problem, minimal per-table code" goal: the only per-table addition is the `stdb_table!`
  line itself — naming the `key` for a PK table, nothing extra for a keyless one.
- **Memory.** Steady state is 1× (no mirror). A reconnect peaks at 2× for one frame, then drops
  `PreviousConnection`. While disconnected (including give-up / explicit disconnect), the baseline holds
  ~1× the last cache until a reconcile or app exit.
- **`stdb_table!` keys on `key`-presence.** `stdb_table!(player => Player, key = identity)` →
  `TableRegistration::pk`; the bare `stdb_table!(monster => Monster)` → `TableRegistration::non_pk`
  (BSATN diff). An optional `[insert, …]` list selects events on either form (default: all three for
  PK, insert+delete for keyless), and `[update]` on the keyless form does not compile. The macro
  generates the PK key extractor and the selection literal; the row-message engine gains a per-table
  resync diff system, gated by the selection and ordered after `StdbSystemSet::RowMessagesPush`, run on
  (resync-in-flight ∧ `is_subscriptions_settled`).
- **Lifecycle change.** `drain_lifecycle_sink` no longer drops `StdbConnection<C>` on `Disconnected`;
  it moves it into `PreviousConnection<C>` (guard: only if `PreviousConnection` is empty, so flapping
  preserves the original baseline). `Connected` writes `StdbConnection<C>` only, never touching the
  baseline. This revises the ADR-0006 disconnect path and the test that asserts "connection removed on
  disconnect."
- **Resync window.** A resync-in-flight flag is set when the baseline is stashed and cleared by the
  fence diff; while set, the per-table row drain discards messages. First connect (no prior disconnect)
  has no baseline, so the flag is false and rows flow normally — resync only engages after a drop. The
  flag is the *presence* of `StdbPreviousConnection<C>` itself — no separate resource — so the baseline
  and the window share one lifetime by construction (the consistency the give-up/clean-slate section
  demands), with nothing to keep in sync.
- **Resync runs in its own ordered phase.** A `Resync` system set is chained **after
  `RowMessagesPush`, before `Main`**, carrying the fence as a single set-level run-condition
  (`StdbPreviousConnection` present ∧ connected ∧ subscriptions settled). The ordering is load-bearing:
  *after `RowMessagesPush`* so the suppressed live drain has already run that frame — dropping the
  baseline earlier would un-suppress the re-fired Snapshot inserts mid-frame and leak them; *before
  `Main`* so reconstructed rows reach the Game's readers the same frame, matching the live path. The
  per-table diffs run **before** the baseline-drop that closes the window (else a diff reads a dropped
  baseline), and both inherit the one set-level gate, so diff and close can never fire under divergent
  conditions. The diff reads the two caches directly (`previous.db().<t>().iter()` vs
  `current.db().<t>().iter()`) — never the row channel, whose re-fired inserts are the suppressed
  Snapshot, not a diff source — keeping it a pure function of the two caches and unit-testable.
- **Free wins.** A Subscription changed *during* the outage (the Game spawns/despawns `Subscription`
  entities) needs no special handling: the diff is "frozen view vs new authoritative", so narrowed
  interest → ghosts → deletes and widened interest → inserts, automatically. No-PK tables get
  clean-rebuild semantics, matching their nature.
- **Supersedes ADR 0002** on reconciliation: the Bridge no longer forwards "only real SDK row events
  with reconciliation left to the Game"; it actively reconstructs the reconnect diff. The opt-in
  clean-slate path preserves the ADR-0002 escape hatch (despawn in a `StdbDisconnected` observer), now
  paired with the baseline reset for consistency.
- **Testable with fakes (ADR 0005).** `FakeConn` can present old/new row sets, so the diff, the
  keep-everything-until-reconcile rule, the flapping guard, and the keyless BSATN diff are unit-testable
  with no socket.
