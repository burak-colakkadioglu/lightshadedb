#!/usr/bin/env bash
# App 3: a connected viewer. It lists the newest notes, or fetches one note
# by id. It never opens the database; reads go through network-host.
set -euo pipefail

app_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$app_dir/../../.." && pwd)"
workload="$root/examples/apps/notes.lightshade"
socket="${LIGHTSHADE_SOCK:-$root/examples/apps/.data/notes.lsdb.sock}"

if [[ -n "${LIGHTSHADE_CLI:-}" ]]; then
    cli=("$LIGHTSHADE_CLI")
else
    cli=(cargo run --quiet --release --manifest-path "$root/Cargo.toml" -p lightshade-cli --)
fi

if [[ ! -e "$socket" ]]; then
    echo "no server socket at $socket" >&2
    echo "start examples/apps/network-host/run.sh first" >&2
    exit 1
fi

if (($# == 0)); then
    exec "${cli[@]}" run "$workload" "$socket" list_notes
else
    exec "${cli[@]}" run "$workload" "$socket" get_note "id=$1"
fi
