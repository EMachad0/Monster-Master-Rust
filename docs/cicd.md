# CI/CD

## When to use

Read this before creating or modifying anything under `.github/workflows/`.

## Workflows

### `ci.yml` (push to main + PRs)

Three jobs:

- **check** — `cargo fmt --check`, `cargo clippy` and `cargo test` over the workspace
  **excluding `stdb_module`**, then `cargo build -p stdb_module --target wasm32-unknown-unknown`
  to confirm the Module compiles to wasm.
- **web** — `trunk build --release`, confirming the client compiles to wasm. Uses the committed
  `stdb_bindings`, so no SpacetimeDB server is needed.
- **bindings** — runs `just stdb::generate` and fails if `crates/stdb_bindings` changes, i.e. the
  committed bindings drifted from the Module.

### `deploy-page.yml` (push to main + manual)

Builds the web client with `trunk build --release --public-url "/<repo>/"` and deploys `dist/` to
GitHub Pages. Requires Pages enabled with **source = GitHub Actions** (repo Settings → Pages).

## Critical rules

### 1. Toolchain comes from `rust-toolchain.toml`, tools from mise

Workflows use `jdx/mise-action@v2` to install `just`, `trunk`, and `spacetimedb-cli` (from
`.mise/config.toml`). Rust is **not** installed by mise — the runner's preinstalled `rustup` reads
`rust-toolchain.toml` and auto-installs the pinned toolchain + the wasm target. Do not add a
separate `dtolnay/rust-toolchain` step; it would bypass the pin.

### 2. The Module is never built by plain `cargo build`/`clippy`

`stdb_module` is excluded from `default-members` and only compiles for `wasm32-unknown-unknown`
(it has unresolved host symbols otherwise). Always pass
`--exclude stdb_module` to workspace-wide cargo commands, and build it explicitly with
`--target wasm32-unknown-unknown`.

### 3. Builds must not require a running server

`stdb_bindings` is committed, so compilation never needs SpacetimeDB. Only the **bindings** job
runs the CLI (to detect drift) — and even that needs no server, since `spacetime generate` builds
and inspects the Module locally.

### 4. Cache Rust builds

Keep `Swatinem/rust-cache@v2` in every job — Bevy is large and uncached CI is slow.

## References

- Local equivalents live in the `justfile` (`just check::all`, `just dev::build`, `just stdb::generate`).
- SpacetimeDB specifics: `docs/spacetimedb.md`.
