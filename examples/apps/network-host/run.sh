#!/usr/bin/env bash
# App 1: the network host. It owns the database file and serves typed
# operations to connected clients over the Lightshade DB wire protocol.
set -euo pipefail

app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$app_dir/../../.." && pwd)"
data_dir="${LIGHTSHADE_DEMO_DATA:-$root/examples/apps/.data}"
workload="$root/examples/apps/notes.lightshade"
database="$data_dir/notes.lsdb"

mkdir -p "$data_dir"

if [[ -n "${LIGHTSHADE_CLI:-}" ]]; then
    cli=("$LIGHTSHADE_CLI")
else
    cli=(cargo run --quiet --release --manifest-path "$root/Cargo.toml" -p lightshade-cli --)
fi

echo "database: $database"
echo "socket:   $database.sock"
echo "press Ctrl-C to stop"
echo

exec "${cli[@]}" serve "$workload" "$database"
