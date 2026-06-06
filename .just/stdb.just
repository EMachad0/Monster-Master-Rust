# SpacetimeDB: build/publish the Module and (re)generate Bindings. Recipes run from the repo root.
set working-directory := '..'

stdb := "mise exec -- spacetimedb-cli"
module := env_var_or_default("SPACETIMEDB_MODULE", "monster-master")
uri := env_var_or_default("SPACETIMEDB_URI", "http://127.0.0.1:3000")

[doc("Build + publish the Module to the SpacetimeDB server (needs the server running)")]
publish:
    {{ stdb }} publish --yes --server {{ uri }} --module-path crates/stdb_module {{ module }}

[doc("Regenerate the committed client Bindings from the Module (no server required)")]
generate:
    rm -rf crates/stdb_bindings/src/module_bindings
    {{ stdb }} generate --lang rust --out-dir crates/stdb_bindings/src/module_bindings --module-path crates/stdb_module --yes

[doc("Publish the Module, then regenerate Bindings")]
setup: publish generate

[doc("Run a read-only SQL query, e.g. just stdb::sql 'SELECT * FROM player'")]
sql query:
    {{ stdb }} sql --server {{ uri }} {{ module }} "{{ query }}"

[doc("Print Module logs; pass -f to follow or -n N to limit, e.g. just stdb::logs -f")]
logs *args:
    {{ stdb }} logs --server {{ uri }} {{ module }} {{ args }}

[doc("Print the Module schema (tables + reducers) as JSON")]
describe:
    {{ stdb }} describe --json --server {{ uri }} {{ module }}

[doc("Call a reducer with JSON args, e.g. just stdb::call set_name '\"Neo\"'")]
call *args:
    {{ stdb }} call --server {{ uri }} {{ module }} {{ args }}
