# Quality gates for the Rust workspace. Recipes run from the repo root.
# The wasm-only Module is excluded from host cargo commands (build it via `just stdb::publish`).
set working-directory := '..'

[doc("Format check + lint + test + confirm the Module compiles to wasm")]
all: fmt lint test module

[doc("Check formatting")]
fmt:
    cargo fmt --all --check

[doc("Lint with clippy (warnings are errors)")]
lint:
    cargo clippy --workspace --exclude stdb_module --all-targets -- -D warnings

[doc("Run tests")]
test:
    cargo test --workspace --exclude stdb_module

[doc("Confirm the Module compiles to wasm (it's excluded from host cargo commands)")]
module:
    cargo build -p stdb_module --target wasm32-unknown-unknown
