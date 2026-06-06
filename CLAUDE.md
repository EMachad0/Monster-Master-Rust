# Monster Master

A multiplayer game: a **Bevy** client (web via WebAssembly + native desktop) on a self-hosted
**SpacetimeDB** backend.

## Tech Stack

- **Client:** Rust + **Bevy 0.18**, running on the web (wasm via **trunk**) and native desktop.
- **Backend:** self-hosted **SpacetimeDB 2.4** (Docker). The Module is written in Rust.
- **Bridge:** `stdb_bevy` — a hand-written, module-agnostic Bevy↔SpacetimeDB integration over
  `spacetimedb-sdk` 2.4. We deliberately do **not** use the `bevy_spacetimedb` crate
  (see `../docs/adr/0001-bevy-spacetimedb-with-hand-written-bridge.md`).
- **Toolchain:** `rust-toolchain.toml` pins Rust (SpacetimeDB 2.4 needs ≥ 1.93); `mise` installs
  `just`, `trunk`, and `spacetimedb-cli`. Dependency versions are centralized in the root
  `Cargo.toml` `[workspace.dependencies]`.

### Workspace (`crates/`)

| Crate           | Role                                                                       |
| --------------- | -------------------------------------------------------------------------- |
| `stdb_module`   | The **Module**: tables + reducers. Built to wasm by the CLI, not host cargo |
| `stdb_bindings` | Generated client bindings. **Committed; never hand-edit** — `just generate` |
| `stdb_bevy`     | The **Bridge** (generic; knows no specific Module)                          |
| `game`          | The Bevy app. `trunk` builds it to wasm; `cargo run -p game` for desktop    |

Glossary of these terms lives in `../CONTEXT.md`.

## Common commands

- `just` — list all recipes (root justfile just imports `.just/` modules)
- `docker compose up` — start SpacetimeDB locally (port 3000)
- `just stdb::publish` — build + publish the Module to the server
- `just stdb::generate` — regenerate `stdb_bindings` (commit the result)
- `just dev::native` — run the native desktop client
- `just dev::web` — serve the web client at http://localhost:8080
- `just dev::clients [N]` — run N native clients at once (default 2; Ctrl-C stops all) for multiplayer testing
- `just check::all` — fmt + clippy + test (skips the wasm-only Module)

## Required Reading

Before starting work, read any docs that match the task at hand:

- Before working on SpacetimeDB (the Module, reducers, the Bridge, bindings, or the client
  connection): read `docs/spacetimedb.md`
- Before creating or modifying CI/CD workflows: read `docs/cicd.md`

## Shell Rules

- **Never use `find -exec`**. It triggers a permission prompt that cannot be auto-allowed. Use one of these alternatives:
  - `find ... -print0 | xargs -0 command` (pipe to xargs)
  - `fd` (already in the allowed list, modern alternative to find)

## Agent Rules

- **Never use the `AskUserQuestion` tool.** If you need clarification, state your assumption and proceed. If you need to present options, list them in plain text output instead.
