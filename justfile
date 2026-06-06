mod wk ".just/wk.just"
mod dev ".just/dev.just"
mod stdb ".just/stdb.just"
mod check ".just/check.just"

set dotenv-load := true

# List all recipes, including those in modules.
default:
    @just --list --list-submodules
