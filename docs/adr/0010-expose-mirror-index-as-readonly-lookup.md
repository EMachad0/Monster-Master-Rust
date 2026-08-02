---
status: accepted (extends ADR 0009)
---

# Exposing the Mirror index as a read-only lookup; despawn stays the Game's

ADR 0009 built a by-key index (`SyncEntityMap<S>`, a `Key` to `SmallVec<[Entity]>` resource kept by
`On<Add>`/`On<Remove>` observers) to drive the **Row mirror**'s update path, and kept it private. But
the Game still needs to go the other way, from a row's key back to its entity: to despawn on a row
delete, and to join one row to another's entity (the cursor reads its owning player's entity to
inherit colour). With the index private, the Game either rescans all entities on every delete (an
O(n) `Query::iter().find`) or hand-maintains its own `Key` to `Entity` map, which is a byte-for-byte
duplicate of the index the Bridge already keeps. This ADR surfaces that index read-only as the
**Mirror index** (see `../CONTEXT.md`), so the Game stops duplicating it, while leaving entity
lifecycle (despawn) the Game's, exactly as ADR 0009 fixed it.

## Considered Options

### Who owns despawn

- **The Bridge auto-despawns on `RowDeleted`** (opt-in per table). Tempting because the Bridge holds
  the index, so it could tear the entity down itself. Rejected: it reverses the central choice of ADR
  0009 (and ADR 0006 before it), that the Bridge syncs components and never owns entity lifecycle.
  Despawn is genuine Game policy: graceful teardown is a server-side soft delete that arrives as an
  update, not a delete (ADR 0009), and one row may back several entities, so "row deleted" does not
  cleanly mean "despawn everything carrying this key." Baking a despawn policy into a module-agnostic
  Bridge is the kind of one-policy lock-in ADR 0002 already refused for reconnect reconciliation.
- **The Game despawns, the Bridge only answers lookups (chosen).** A faithful continuation of ADR
  0009: the Bridge attaches and maintains components, and now answers "which entities carry key K";
  the Game keeps owning spawn and despawn. Once the lookup is O(1), the Game's despawn shrinks to a
  three-line reaction, so almost no boilerplate is left to remove by going further. The lookup is
  needed anyway for the cross-table join, which only the Game can act on, so exposing it is mandatory
  regardless of who despawns.

### How the index is exposed

In Bevy 0.19 `Resource: Component`, so the index does carry a mutability marker, but immutability is
the wrong tool: the `On<Add>`/`On<Remove>` observers must mutate it, so it cannot be immutable to
everyone. What we want is "mutable inside the Bridge crate, unreachable outside," which is plain Rust
visibility.

- **Make `SyncEntityMap<S>` public** with read-only methods. Rejected: once the type is nameable, any
  Game system can request `ResMut<SyncEntityMap<S>>` and corrupt an invariant the observers depend
  on, and even a read-only-intentioned `ResMut` would make the scheduler serialise that system
  against the observers (write-lock contention).
- **A read-only `SystemParam` wrapping the crate-private resource (chosen).** `SyncEntityMap<S>` stays
  crate-private; the only public handle is `RowEntities<'w, S>`, whose sole field is a private
  `Res<SyncEntityMap<S>>`. The Game cannot name the resource, so it can request neither `Res` nor
  `ResMut` of it, and its lookups always run as parallel reads. This is the same encapsulation the
  Bridge already applies to its other observer-maintained state.

### What a lookup returns, and how it is keyed

- The primitive is `get(key) -> &[Entity]`, returning an empty slice for an absent key, so a despawn
  loop and a `.first()` both fall out cleanly and no single-vs-many policy is baked in.
- `single(key) -> Result<Entity, QuerySingleError>` is added for the 1:1 join (zero, one, many map to
  `NoEntities` / `Ok` / `MultipleEntities`). It reuses Bevy's own `QuerySingleError` rather than a
  bespoke type so the Game gets back exactly what it already matches from `Query::single`; the only
  cost is a query-worded `Display` that is cosmetic in logs.
- `get_by_row(row)` / `single_by_row(row)` are sugar that derives the key from a deleted row via
  `S::from(row).key()`, because a despawn reaction holds the row, not the key, and there is no generic
  row-to-key projection (the same wall recorded in the `table_pk` investigation; only codegen would
  remove it). The sugar keeps the awkward, unintuitive conversion out of the Game's despawn sites.
- The index is keyed on `S::Key`, so the Mirror index covers only tables the Game **mirrors**.
  Keyless tables (the `non_pk` path, ADR 0007) carry no key and get no Mirror index. This is forced,
  not a gap: a keyless row has no stable identity to look up by.

### The failure mode of a missed lookup

Whether a missing entity is a panic or a skipped frame is left to the Game: `single` returns a
`Result`, and the Game decides `expect` versus `else { continue }`. The Bridge stays agnostic, in
keeping with the division of labour above.

## Consequences

- New public surface: a `RowEntities<'w, S: StdbSync>` `SystemParam` with `get` / `single` (keyed) and
  `get_by_row` / `single_by_row` (row-sugar). `SyncEntityMap<S>` stays crate-private.
- The Game drops its hand-kept `Key` to `Entity` map (`PlayerIdentityMap`) entirely: the player and
  cursor despawns and the cursor-to-player join all read `RowEntities` instead. The O(n) cursor
  despawn scan is gone.
- Lookups are available the same frame the entity is, because the index is populated by the same
  command flush that materialises the entity into its archetype; a join that already relied on
  querying the spawned entity sees the index ready at the same point.
- `get_by_row` / `single_by_row` build and drop one mirror component per delete to read its key (for
  `Player`, that clones a `String` it discards). This is once per delete event, which is rare, so it
  is paid for ergonomics, not on any hot path.
- The same `get` / `single` primitives let the Game do a find-or-spawn upsert (look up by key, spawn
  when `NoEntities`), which is the "several rows, one entity" follow-up ADR 0009 left out of scope,
  now expressible Game-side with no Bridge helper.
- The glossary gains **Mirror index**; ADR 0009's "an index keyed for O(1) lookup is a possible later
  optimisation" and its find-or-spawn follow-up are the threads this picks up.
