# Client: run and build (native + web). Recipes run from the repo root.
set working-directory := '..'

trunk := "mise exec -- trunk"

[doc("Run the native desktop client")]
native *args:
    cargo run -p game {{ args }}

[doc("Serve the web client at http://localhost:8080 (rebuilds on change)")]
web *args:
    {{ trunk }} serve {{ args }}

[doc("Build the web client for production into dist/")]
build *args:
    {{ trunk }} build --release {{ args }}

[doc("Run N native clients at once (default 2); Ctrl-C or closing the shell stops them all")]
clients count="2":
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -p game
    pids=()
    cleanup() { trap - EXIT INT TERM HUP; kill "${pids[@]}" 2>/dev/null || true; }
    trap cleanup EXIT INT TERM HUP
    for i in $(seq 1 {{ count }}); do
        ./target/debug/game &
        pids+=("$!")
    done
    echo "spawned {{ count }} client(s): ${pids[*]} — Ctrl-C to stop all"
    wait
