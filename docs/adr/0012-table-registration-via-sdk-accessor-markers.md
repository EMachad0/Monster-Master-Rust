---
status: accepted (supersedes the "the generic accessor form is inexpressible without leaking the connection" stance of ADR 0004)
---

# Table registration via the SDK's accessor markers

`stdb_table!` used to paste concrete per-table code (`conn.db().player()`) into the Game's crate,
because a generic accessor was believed to require a `'static` db view and therefore a leaked
connection. SpacetimeDB SDK 2.7 adds `TableAccessor`, whose generic associated type makes the
borrowed handle nameable, so registration now takes the generated marker as a type parameter:
`stdb_table!(PlayerTableAccessor, key = identity)`. The macro no longer emits the forward or
snapshot closures, no longer takes a row type, and no longer passes a log label.

## What changed in the SDK

Verified SDK facts (spacetimedb-sdk 2.7.0):

- `spacetimedb_sdk::table::TableAccessor<DbView>` carries `type Row`, a **generic associated type**
  `type Handle<'db> where DbView: 'db`, and `fn get<'db>(db: &'db DbView) -> Self::Handle<'db>`.
- Codegen emits one zero-size marker per table, `{TableNamePascalCase}TableAccessor`, whose `get`
  body is the accessor call itself (for the `player` table: `db.player()`).
- The SDK documents these markers as existing for downstream Bevy bridges, to simplify table
  registration.

The generic associated type is the whole difference. ADR 0004 rejected the accessor-function-pointer
form because `for<'a> Fn(&'a DbView) -> Handle<'a>` could not be written, leaving `'a = 'static` and
a leaked connection as the only route, which ADR 0002 had already ruled out. With the handle type
reachable as `A::Handle<'db>`, the transient-borrow form is expressible as a higher-ranked bound:

```rust
for<'db> A::Handle<'db>: WithInsert<Row = R> + WithDelete<Row = R> + WithUpdate<Row = R>
```

`A` is a type parameter, so dispatch is static and `Table`'s lack of object safety never applies. No
`'static` db view, so no leak. Verified by compiling and running this shape before adopting it.

## Considered Options

### Where the `DbContext` bound lives

- **On `StdbConn` itself** (`trait StdbConn: DbContext + 'static + Send + Sync`), dropping the
  blanket impl. Rejected: `FakeConn` has no `DbContext` impl and no use for one, yet it backs every
  connection, reconnect, subscription, and lifecycle test; and `docs/testing.md` documents `StdbConn`
  as asking only for `Send + Sync + 'static`, which is the stated reason the engine layer is testable
  without a socket. Requiring the SDK trait on the Bridge's connection abstraction would turn a
  tables-only need into a global constraint.
- **On the `TableRegistration::pk` / `non_pk` constructors only (chosen).** They are the only
  functions that name `A`. Two generic free functions, instantiated as plain `fn` pointers, carry the
  marker across the boundary, so `TableRegistration<C>`'s type, `add_stdb_table`,
  `resync_row_messages_system`, `drain_row_sink`, `RowForwarder`, and `StdbConn` all stay free of it.
  The tables path already has its `DbContext` fake (`FakeDbContext`), so no new test double is needed.

### How the Game names a table

- **Keep the accessor identifier and add the marker** (`stdb_table!(player: PlayerTableAccessor, ..)`).
  Rejected: two names for one table, and a longer call site than before.
- **Synthesize the marker name from the accessor identifier** with `paste!` or a proc macro, keeping
  today's `stdb_table!(player => Player, ..)` byte for byte. Rejected: it hard-codes codegen's
  snake-case to PascalCase naming inside the Bridge, and buys back only the log label.
- **The marker alone (chosen).** `stdb_table!(PlayerTableAccessor, key = identity)`. The row type
  token is dropped because it was never name-resolved: it appeared only in the macro's patterns and
  in no expansion, so `stdb_table!(player => Player, ..)` compiled in a scope where `Player` was not
  in scope at all, while an unrelated `Player` component existed in the Game. `TableAccessor::Row`
  supplies the row type for real. The marker is taken as an `ident`, not a `ty`, so the Game imports
  it and one table always yields one label.

### The `table` log field

- **Thread the accessor name from the macro** (`stringify!`), as before. Rejected: it keeps a
  parameter alive through `TableRegistration::pk`, `add_stdb_table`, `resync_row_messages_system`,
  and `drain_row_sink` purely to carry a string, and with the marker as the only token it would read
  `PlayerTableAccessor`.
- **Derive it from the row type (chosen).** Both consumers are already generic over `R`, so the label
  needs no parameter: it is the last path segment of `std::any::type_name::<R>()`, computed once when
  the system is built. This is faithful because `R` already identifies a registration uniquely:
  `RowChannel<R>` is a Bevy resource keyed on `R` alone, so two tables sharing a row type would
  already have one silently overwrite the other's channel.

## Consequences

- **Supersedes ADR 0004 on one rejected option only.** Its decision stands unchanged: registration is
  still declared on the plugin via a macro, and still re-run on every `StdbConnected`, for the reasons
  that ADR gives. What is obsolete is the claim that the generic form requires a leak. The parallel
  remark in ADR 0002 is obsolete for the same reason.
- **The Bridge now requires spacetimedb-sdk 2.7 or later** in its core registration path, not merely
  in the SDK adapter. `TableAccessor` has no fallback.
- **The `table` log field changes meaning**, from the SQL accessor name (`player`) to the row type
  name (`Player`). It is still one token per registration, and it is the PascalCase of the SQL name,
  but it is no longer byte-identical to it. `docs/observability.md` records this. Its exact text now
  comes from `std::any::type_name`, whose output std does not guarantee across compiler versions;
  taking the last path segment is what keeps that survivable.
- **The Game no longer imports the per-table accessor extension traits.** `PlayerTableAccess` and
  `CursorTableAccess` existed only to satisfy the macro's hidden `conn.db().player()`, so forgetting
  one produced a "no method named `player`" error pointing into an expansion. The Game now imports
  the marker types it writes itself.
- **`add_tables` inference is load-bearing and now rests on the array alone.** ADR 0004 pinned `C`
  through the bare `|conn, fwd|` closures; those are gone, and `C::DbView` does not invert, so `C`
  must be fixed by the `[TableRegistration<Cd::Conn>; N]` element type before the constructor's
  bounds are checked. The existing test that names the connection type zero times is the guard.
- **`key =` survives.** `TableAccessor::Row` carries no primary-key projection, and 2.7 adds no
  `TableWithPrimaryKey::primary_key(row)`, so the Resync key still has to be named. This refactor
  removes two of the macro's three closures, not all three.
