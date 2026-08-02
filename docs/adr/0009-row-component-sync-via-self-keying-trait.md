---
status: accepted
---

# Syncing server rows into ECS components via a self-keying `StdbSync` trait, lifecycle left to the Game

The Bridge already turns subscribed rows into buffered messages (`RowInserted` / `RowUpdated` /
`RowDeleted<T>`, per ADR 0002/0004) and reconstructs a correct diff across reconnects (ADR 0007).
Today every Game hand-writes the systems that turn those messages into entity work: spawn on insert,
find-the-entity-and-update on change, despawn on delete, plus the bookkeeping to correlate a row to
its entity. This ADR adds an opt-in, module-agnostic layer (a **Row mirror**, see `../CONTEXT.md`)
that keeps declared ECS components in step with a table's rows, so the Game stops hand-writing the
per-table update loop and gets correct change detection for free.

The investigation started from a different idea: register Bevy 0.19 **BSN** templates and let the
Bridge spawn/patch whole entities. That idea was walked back in stages; the constraints below are what
forced the final, much smaller shape.

## Forces

These are properties of Bevy 0.19 and the existing Bridge that constrain any solution. They were each
verified against the resolved Bevy 0.19 source and the Bridge code.

- **The orphan rule permits a conversion when a local type appears anywhere in the impl head**, not
  only as the `Self` type. So in the Game crate: `impl From<&Player> for Position` is legal (`Position`
  is local), and `impl From<Position> for Transform` is *also* legal (`Position` is local, even though
  `Transform` is foreign), but `impl From<&Player> for Transform` is an orphan violation (both
  `Player` and `Transform` are foreign). This is the single fact that shapes the conversion surface.
- **`StdbRow` is a blanket trait bounded `: 'static`** (`impl<R: 'static + Send + Sync + Clone +
  PartialEq> StdbRow for R`). It carries no associated `Key`, a per-type `impl StdbRow for Player` is
  impossible (it overlaps the blanket and is an orphan in the Game), and the generated row does not
  mark its primary-key field (ADR 0007). So there is **no generic way to extract a row's key**, and a
  key type cannot hang off the row.
- **Bevy 0.19 has no by-value component index.** The only `component_index` is archetype-level; there
  is no "find the entity whose component value equals K" lookup. Correlating a row to its entity
  therefore requires the key to be carried, queryable, on the entity itself.
- **BSN `apply_scene` is a blind overwrite.** It rebuilds every templated component and writes them
  via a plain insert (`insert_by_ids_internal`), marking each `Changed` with no equality check, and a
  resolved `Scene` hides its component types (it cannot be diffed or partially re-applied). There is no
  retained-instance diff. So BSN cannot give per-component change detection and cannot be the sync
  mechanism.
- **Resync re-emits the row messages.** ADR 0007 reconstructs the reconnect diff through the same
  `RowInserted` / `RowUpdated` / `RowDeleted<T>` types, so any layer that reacts to those messages
  inherits correct reconnect behaviour with no extra code.

## Considered Options

### Who owns the entity

- **The Bridge owns whole mirror entities** (spawn on insert, despawn on delete). Rejected: it inverts
  ADR 0006 (the Game owns entity lifecycle; the Bridge owns only the components it attaches), it forces
  BSN `apply_scene` and its blind-overwrite/`Changed` problems, and it cannot express a row whose data
  belongs on an entity the Game composes, nor several rows feeding one entity.
- **The Game owns the entity; the Bridge syncs declared components (chosen).** A faithful extension of
  ADR 0006: widen "the components the Bridge attaches" from markers to row-mirrored components. The
  Game spawns (in its own `RowInserted` reaction, which is the legitimate entity template, not
  boilerplate), despawns (its own `RowDeleted` reaction), and freely owns every other component. The
  Bridge only keeps the declared components in step. This is what makes **one row feeding components on
  several entities** expressible, which the entity-owning model could not.

### How the conversion is declared

- **A single `bsn!` scene per row.** Rejected per the BSN force above: opaque, blind overwrite,
  over-fires `Changed`.
- **A closure registered at plugin setup (`fn(&Row) -> T`).** Works for any target including foreign
  components, but puts the conversion away from the component and (as first framed) duplicated it
  between the spawn and the registration.
- **A trait on the component (chosen).** `StdbSync` plus `From<&Self::Row>`, both implemented on the
  Game's own (local) component, so the conversion is co-located with the component and written once.
  The orphan force makes this legal for owned components, and the same `From` is reused by the Game's
  spawn and the Bridge's update, so there is no duplication.

### How a row is correlated to its entity

A key must live on the entity (no by-value index). This yields a hard either/or, call it the no-tag
theorem: you cannot have both no correlation tag and direct sync into a foreign component, because a
foreign component (`Transform`) cannot carry a key field, so dropping the tag requires the synced
component to carry its own key, which only owned components can do.

- **A separate key tag component** (`SyncedFrom<R>(key)`) maintained by lifecycle hooks. Rejected as
  the default: it is forgettable, it parks the key off the component, and it needs an index plus
  add/remove hooks plus despawn cleanup.
- **Self-keying (chosen).** The synced component carries its own key and reports it via `key()`. The
  update system scans the component and matches by key, so there is no tag, no index, no hooks, and
  despawn needs no cleanup (it is stateless). The accepted price: each synced component carries a key
  field, and a foreign component cannot self-key, so foreign targets are reached two-hop.

### Foreign targets (`Transform`, `Sprite`, ...)

Direct sync into a foreign component is impossible (the orphan force: `From<&Player> for Transform`
does not compile). The chosen route is a **two-hop derive**: sync the row into an owned domain
component (`Position`), then project that into the foreign component with `project_into::<Position,
Transform>()`, backed by `impl From<Position> for Transform` (legal, `Position` is local, so
`Position: Into<Transform>` follows from the standard blanket). The projection runs gated on
`Changed<Position>` and set-if-changes `Transform`. This is a clean domain/render separation, not the
per-change copy it superficially resembles, and the conversion stays co-located with the owned
component.

### Change detection

- **Blind re-apply** (rebuild and overwrite every update). Rejected: it marks every component
  `Changed` even when the value did not change, which defeats `Changed<T>`-driven systems such as
  animation.
- **Set-if-changed (chosen).** Build the candidate from the row, compare via `PartialEq`, and write
  only on a real difference. `Changed<T>` then fires only on a genuine change, on both the synced
  component and the projected target.

### Building from the row: owned vs borrowed

The row arrives behind a reference (`&RowUpdated<R>`). `From<Self::Row>` would force a clone per
changed row. `From<&Self::Row>` avoids it but needs a lifetime, so the supertrait is **higher-ranked**:
`for<'a> From<&'a Self::Row>`. This is well-formed precisely because `StdbRow: 'static` makes `&'a
Self::Row` valid for every `'a`. Verified to compile and run. The Game writes `impl From<&Player> for
Position` (elided lifetime); the Bridge builds `T::from(&row)` with no clone, and `Clone` is not
required on the synced component (the system rebuilds from the borrowed row per matched entity).

### Scheduling

- **Dedicated `Sync` / `Project` system sets.** Rejected as unnecessary, and `Sync` collides in name
  with `Resync`.
- **In `Main` (chosen).** `RowUpdated<R>` is written by `RowMessagesPush` and `Resync`, both chained
  before `Main`, so sync systems in `Main` see this frame's messages (live and reconnect-diff). A
  projection runs `.after` its source's sync system. The Game orders syncs however it likes; the Bridge
  documents the dependencies (sync after the row messages, projection after its sync) rather than
  forcing an order.

### Resources

Out of scope. Noted for the future: in Bevy 0.19 `Resource: Component`, so a resource is a component on
a backing entity; if revisited, it would be an extension of `sync_component`, not a separate function.

## Consequences

- **New public surface.** A `StdbSync` trait:

  ```rust
  pub trait StdbSync: Component + PartialEq + for<'a> From<&'a Self::Row> {
      type Row: StdbRow;
      type Key: Eq + Hash + Clone;
      fn key(&self) -> Self::Key;
  }
  ```

  plus `sync_component::<T>()` (one type parameter, recovered from `T`) and `project_into::<R, T>()`
  (`R: StdbSync + Into<T>`), both adding systems to `Main`.
- **Division of labour.** The Game owns lifecycle: it spawns in its `RowInserted` reaction (writing
  initial values with the same `T::from(&row)` the Bridge uses, so the conversion lives once in the
  impl), and despawns in its `RowDeleted` reaction. Graceful teardown (death animations) is a
  server-side soft delete: keep the row, flip a flag, which arrives as an update. The Bridge owns only
  the update sync.
- **Self-keying.** Each synced component carries a key field, populated by `from`, read by `key()`.
  This is the accepted cost of having no correlation tag, and per the no-tag theorem it cannot be
  removed by any row-side key extraction. This key correlates a row to its entities, a job entirely
  separate from the key the reconnect diff uses to classify rows (ADR 0007): it must be unique per row
  and immutable for the row's lifetime, but it need not be the table's primary key and does not have to
  match the diff key. The two are independent choices that happen to coincide when a table keys both on
  its primary key.
- **Stateless and reconnect-correct.** No index, no tag, no hooks; the sync system scans by key
  (O(n) over entities carrying the component, once per frame that has updates). An index keyed for
  O(1) lookup is a possible later optimisation. Resync composes untouched: its re-emitted `RowUpdated`
  diff flows straight through `sync_component`.
- **Correct change detection.** Set-if-changed means `Changed<T>` fires only on a real change, which
  is what makes the synced components usable as animation triggers.
- **One row may feed several entities, and several rows may feed one entity.** Both fall out of the
  Game owning composition and the Bridge syncing per-component.
- **BSN is not adopted.** It is at most an ingredient (a `Template` could build a synced component that
  needs world context, e.g. an asset handle), not the mechanism. The single-scene, entity-owning, and
  `apply_scene` approaches are all rejected above.
- **Out of scope (deliberate follow-ups).** A `#[derive(StdbSync)]` to generate `from`/`key`;
  resources; merging the `Resync` set into the row-messages set; a by-key index optimisation; and any
  find-or-spawn helper for the several-rows-one-entity case.

## Implementation notes (as built)

Two choices during implementation departed from the design above; both were deliberate and are
recorded here so the ADR matches the code.

- **The by-key index was pulled forward, not deferred.** The Consequences above list an O(1) by-key
  index as a possible later optimisation, and "Out of scope" lists it as a follow-up. It was built
  now instead: a `SyncEntityMap<S>` resource (`Key` to `SmallVec<[Entity; 4]>`) maintained by
  `On<Add>` / `On<Remove>` observers, so the update system looks the entity up by key rather than
  scanning. This reintroduces the index and the add/remove hooks the stateless scan avoided, and adds
  one bound to the trait, `Key: Send + Sync`, because the key is now stored in a resource (the
  transient scan never required that). The one-row-many-entities property is preserved by the per-key
  `SmallVec`. This is what forces the key's immutability: the map is populated only by the `On<Add>` /
  `On<Remove>` observers, while the update path writes in place with set-if-changed and fires neither,
  so a key that changed value on an update would leave the entity filed under its old bucket and
  unreachable by later updates.
- **The projection updates an existing target and does not insert it.** The Foreign targets section
  has the projection set-if-change the target; as built it does not create the target when it is
  absent. The Game composes the entity with the target component (consistent with the Game owning
  composition); a projection that finds no target logs a warning rather than inserting one.
