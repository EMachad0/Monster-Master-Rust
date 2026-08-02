# Monster Master

A multiplayer game built as a Bevy client (web via WebAssembly + native desktop) talking to a
self-hosted SpacetimeDB backend. This glossary fixes the project's ubiquitous language so the
same concept always has the same name in code, docs, and conversation.

## Language

**Module**:
The SpacetimeDB server-side database logic — table definitions plus reducers — compiled to a wasm
`cdylib` and published to the SpacetimeDB server. It is the authoritative source of game state.
Lives in the `stdb_module` crate.
_Avoid_: schema, server, backend, database (each names only one facet of the Module).

**Reducer**:
A transactional function inside the Module that mutates database state. The only way a client is
allowed to change authoritative state.
_Avoid_: handler, endpoint, RPC, command.

**Reducer outcome**:
Whether a **Reducer** this client invoked committed or failed, surfaced by the **Bridge** to the
**Game** as a typed event it can react to. Covers only the caller's own calls.
_Avoid_: response, ack, result, callback.

**Bindings**:
The generated Rust client types for a specific Module (the concrete connection, table, and reducer
types), produced by `spacetime generate`. Committed to the repo and regenerated, never hand-edited.
Lives in the `stdb_bindings` crate.
_Avoid_: codegen, stubs, types, glue.

**Bridge**:
The module-agnostic integration between SpacetimeDB and Bevy: it advances the connection each frame
and turns connection lifecycle and table/reducer changes into Bevy events and resources. Knows
nothing about any specific Module.
Lives in the `stdb_bevy` crate.
_Avoid_: plugin, adapter, client, SDK wrapper.

**Game**:
The Bevy application — the playable client, compiled to both web (wasm) and native desktop. Depends
on the Bridge and the Bindings.
_Avoid_: client, frontend, app.

**Subscription**:
A query whose matching rows the server replicates into the **Client cache** — *what data the Game
is currently fetching*. A **Bridge** concern: the Game declares the desired queries, but the Bridge
owns the subscription lifecycle and re-applies it on every (re)connect. Changeable at runtime.
_Avoid_: fetch; query (the SQL is only part of it).

**Client cache**:
The SpacetimeDB SDK's local replica of the subscribed rows, read via `conn.db()`. It is **rebuilt
empty on every reconnect** (the SDK has no resume; the Bridge builds a fresh connection), so a
reconnect re-delivers the whole **Snapshot** as inserts and never signals deletions that happened
during the outage.
_Avoid_: cache, store, local DB (each is ambiguous on its own).

**Snapshot**:
The full set of currently-matching rows the server sends when a Subscription is applied. After a
reconnect the Snapshot is the only authoritative state the Game receives.
_Avoid_: dump, initial state.

**Resync**:
Bringing the Game's view back in line with the Snapshot after a reconnect: refresh surviving rows
and drop **Ghost rows**. Owned by the Bridge.
_Avoid_: reconciliation (fine in prose), refresh, sync.

**Ghost row**:
A row deleted while the Game was disconnected that lingers in the Game's view because the reconnect
Snapshot only re-inserts survivors and never signals the deletion. Resync is what removes them.
_Avoid_: stale row, orphan.

**Resync fence**:
The condition that every active **Subscription** has re-applied (or terminally errored) after a
reconnect. **Resync** must wait for it before dropping **Ghost rows**, because sweeping while a
Subscription is still loading would delete rows that simply have not arrived yet.
_Avoid_: barrier, gate.

**Resync key**:
The per-row identifier the Bridge diffs the pre-outage rows against the reconnect **Snapshot** by, to
classify each row as an insert, an update, or a **Ghost row**. Must be unique per row and stable
across the outage, so a surviving row keys the same before and after rather than reading as a deletion
plus a re-insert. Usually the table's primary key. Distinct from the **Mirror key**: the Resync key
decides *what changed*, never *which entity holds it*.
_Avoid_: sync key; primary key (the Resync key names the role, not the column).

**Connection intent**:
What the Game wants the connection to be — *connected* or *disconnected* — as distinct from what it
currently *is*. The Bridge tracks intent separately from the live connection status so it can tell a
deliberate disconnect apart from a dropped one. Driven by the Game's connect/disconnect requests,
not by the connection's actual state.
_Avoid_: state, mode (the connection's actual state is its *status*, a separate idea).

**Unintended disconnect**:
A connection drop that occurs while the **Connection intent** is still *connected* — the Game never
asked to disconnect, so the link was lost (server restart, network loss, idle timeout). This is the
condition that arms a reconnect; a disconnect that matches a *disconnected* intent is deliberate and
arms nothing. The distinction is what stops the Bridge from fighting a user-requested disconnect.
_Avoid_: failure, error (an unintended disconnect is normal and expected, not an error).

**Identity**:
A player's stable cryptographic identity, issued by SpacetimeDB and carried in the connection's
auth token (a JWT). The **Bridge** surfaces it, once the server issues it on connect, as a resource
the Game reads to answer *"is this mine?"* without depending on the concrete connection type. It is
the same across a same-session reconnect (the Bridge reuses the token in memory), and is distinct
from the connection's *status* (whether the link is up) and its **Connection intent** (whether the
Game wants it up). A **Player** row is keyed by its Identity.
_Avoid_: connection id (a separate, per-connection value), session, user, account.

**Table registration**:
Opting a table into the Bridge's row-change events so its changes surface to the Game — *"this
table exists and I want its callbacks."* Declared statically on the plugin.
_Avoid_: subscribing (a Subscription is a separate concern).

**Row mirror**:
Keeping one or more ECS components in step with a server row, on entities the Game spawns and owns.
The Game tags an entity as backed by a given row (carrying that row's key); the Bridge then writes
those components from the row's current value whenever it changes, with correct change detection,
across every entity tagged with that key. The Bridge syncs only those components and never spawns,
despawns, or otherwise owns the entity — lifecycle (spawn on row insert, teardown on row delete) is
entirely the Game's, driven from the raw row-change events. One row may back components on several
entities. A higher-level alternative to the Game hand-writing per-row update systems. Distinct from
**Table registration** (which only decides whether changes surface) — a mirror is opted in on top
of a registered table.
_Avoid_: projection (fine in prose), replication, binding.

**Mirror index**:
The Bridge's read-only lookup from a **Mirror key** to the ECS entities currently carrying that
mirrored component. The Bridge already keeps this index to drive the mirror; surfacing it read-only
lets the Game find the entity (or entities) backing a row, for instance to despawn when the row is
deleted or to join one row to another's entity, without maintaining its own row->entity map. Keyed
by the **Mirror key**, so it only covers tables the Game mirrors.
_Avoid_: map, cache (the Game's old hand-kept HashMap is exactly what this replaces); component
index (a separate, archetype-level Bevy notion).

**Mirror key**:
The identifier a **Row mirror** correlates a row to its entities by: the value a mirrored component
reports so the Bridge can find every entity backed by that row. Must be unique per row (distinct rows
get distinct keys; one row backing several entities is expected and shares a key) and immutable for
the row's lifetime, since the Bridge files entities under this value and never re-files them when it
changes. Independent of the **Resync key**: it names an entity, not a diff, and need not be the
table's primary key.
_Avoid_: sync key, component id; primary key (the Mirror key may differ from it).

**Cursor**:
A player's live pointer position, replicated to every player so each sees the others, drawn as a
colored circle. A per-player record distinct from the OS pointer the window reports.
_Avoid_: pointer, mouse (those name the local OS input, not the shared replicated position).

**Palette**:
The predetermined, ordered list of display colors the Game draws from, held client-side.
_Avoid_: theme, colours.

**Color slot**:
A player's stable index into the **Palette**, assigned in join order and wrapping when players
outnumber colors.
_Avoid_: color (the Player's stored `color` is this slot, an ordinal, not an RGB value).

## Flagged ambiguities

- "subscription" and "table" were used interchangeably — resolved: a **Subscription** controls
  *which rows flow into the cache* (dynamic); **Table registration** controls *whether those changes
  surface as Bevy messages* (static). They are independent.
- Subscription ownership moved from the Game to the **Bridge** (supersedes ADR 0003's "Subscription
  is a Game concern", which was an initial complexity-avoidance choice). The Bridge needs to own the
  subscription to capture its `on_applied` as the **Resync** fence and to re-apply on reconnect.
- "key" was overloaded across two unrelated jobs, now split into the **Resync key** (diffs snapshots
  on reconnect; unique and stable per row) and the **Mirror key** (correlates a row to its entities;
  unique per row and immutable). They may be the same column but need not be, and neither derives from
  the other.
- A **Reducer outcome** only ever covers the caller's own calls. SpacetimeDB does not deliver other
  clients' reducer runs to this client (no identity or arguments on the wire), so broadcast or
  ephemeral player feedback (an emote, a spell cast, transient chat) is modeled as a **Row**, not a
  Reducer outcome.
