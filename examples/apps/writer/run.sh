#!/usr/bin/env bash
# App 2: a connected writer. It never opens the database; every operation
# travels over the socket owned by network-host.
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

if (($# == 0)); then
    read -r -p "Title: " title
    read -r -p "Body:  " body
else
    title="$1"
    shift
    body="$*"
fi

if [[ -z "$title" ]]; then
    echo "title cannot be empty" >&2
    exit 1
fi

id="${LIGHTSHADE_DEMO_ID:-$(date +%s%N)}"
created_at="${LIGHTSHADE_DEMO_CREATED_AT:-$(date +%s)}"

"${cli[@]}" run "$workload" "$socket" add_note \
    "id=$id" \
    "title=$title" \
    "body=$body" \
    "created_at=$created_at"

echo "saved note $id"
