# Monster Master

A multiplayer game: a **Bevy** client (web via WebAssembly + native desktop) on a self-hosted
**SpacetimeDB** backend.

## Tech Stack

- **Client:** Rust + **Bevy 0.19**, running on the web (wasm via **trunk**) and native desktop.
- **Backend:** self-hosted **SpacetimeDB 2.7** (Docker). The Module is written in Rust.
- **Bridge:** `stdb_bevy` — a hand-written, module-agnostic Bevy↔SpacetimeDB integration over
  `spacetimedb-sdk` 2.7. We deliberately do **not** use the `bevy_spacetimedb` crate
  (see `../docs/adr/0001-bevy-spacetimedb-with-hand-written-bridge.md`).
- **Toolchain:** `rust-toolchain.toml` pins Rust (SpacetimeDB 2.7 needs ≥ 1.93); `mise` installs
  `just`, `trunk`, `spacetimedb-cli`, and `worktrunk` (`wt`, the worktree manager). Dependency
  versions are centralized in the root `Cargo.toml` `[workspace.dependencies]`.

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
- `just wk::new_ui <branch>` — create a worktree off origin/main and open a zellij dev tab (worktrunk-backed; `wk::new` skips the tab, `wk::list` / `wk::rm` inspect or remove)
- `docker compose up` — start SpacetimeDB locally (port 3000)
- `just stdb::publish` — build + publish the Module to the server
- `just stdb::generate` — regenerate `stdb_bindings` (commit the result)
- `just dev::native` — run the native desktop client
- `just dev::web` — serve the web client at http://localhost:8080
- `just dev::clients [N]` — run N native clients at once (default 2; Ctrl-C stops all) for multiplayer testing
- `just check::all` — fmt + clippy + test (skips the wasm-only Module)

## Required Reading

These docs are **not** optional background. When a task matches one, you **MUST read the whole doc
before writing any code** — they encode conventions you are expected to follow, not summarize.

- Before writing or changing **tests**, or designing code that must be testable: read `docs/testing.md`
- Before working on **SpacetimeDB** (the Module, reducers, the Bridge, bindings, or the client
  connection): read `docs/spacetimedb.md`
- Before creating or modifying **CI/CD** workflows: read `docs/cicd.md`
- Before adding or changing **observability/logging** in the Bridge: read `docs/observability.md`

## Skills

Invoke these with `/<name>`. When a request matches one, use it rather than improvising.

- **`dev-tdd`** — collaborative, pair-style TDD: you write the tests, the developer writes the
  implementation, moving one vertical slice at a time. Use when building a feature or fixing a bug
  together, test-first.
- **`grill-with-docs`** — relentless interview that stress-tests a plan against the project's
  language and documented decisions. Use before building, to settle a fuzzy design.
- **`handoff`** — compact the current conversation into a handoff document for another agent. Use
  when wrapping up or passing work along.

## Shell Rules

- **Never use `find -exec`**. It triggers a permission prompt that cannot be auto-allowed. Use one of these alternatives:
  - `find ... -print0 | xargs -0 command` (pipe to xargs)
  - `fd` (already in the allowed list, modern alternative to find)

## Comment Rules

When writing new comments:

- Sound **professional** and follow the existing codebase's comment style.
- Keep them **concise**.
- Comment **what is not easily inferred from the code** — the *why*, a non-obvious constraint, a
  subtle edge case — not what the code already says plainly.
- **Never reference untracked files** (e.g. ADRs, `CONTEXT.md`). They live outside the repo's
  working tree, so such references dangle for anyone without the parent layout. (This applies to
  *code comments* only — docs may describe those decisions in their own words.)

## Always Check Your Work

- After writing or changing any code, run **`just check::all`** (fmt + clippy + test + Module wasm
  build) and confirm it passes. It is how you verify the code matches your intent — do not consider
  work done until it is green.

## Git Rules

- **Never commit your own code before the developer has reviewed it.** Present the work for review
  first; commit only after it has been reviewed (the `dev-tdd` per-slice commit follows this — the
  developer's review is the gate).
- **Check your work before committing** — run `just check::all` and confirm it passes first.
- **Rebase** → always `git fetch` first, then rebase.
- **Open a PR** → `git fetch` → rebase → stage → commit → push → create the PR, in that order.

## Agent Rules

- **Never use the `AskUserQuestion` tool.** If you need clarification, state your assumption and proceed. If you need to present options, list them in plain text output instead.
