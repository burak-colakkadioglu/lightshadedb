#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WORK="${LIGHTSHADE_RESTORE_DRILL_DIR:-$(mktemp -d)}"
KEEP="${LIGHTSHADE_RESTORE_DRILL_KEEP:-0}"
CLI="${LIGHTSHADE_RESTORE_DRILL_CLI:-$ROOT/target/release/lightshade}"
WORKLOAD="examples/messages.lightshade"
DATABASE="$WORK/messages.lsdb"
BACKUP="$WORK/backup.lsdb"

cleanup() {
    if [[ "$KEEP" != "1" ]]; then
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

if [[ ! -x "$CLI" ]]; then
    cargo build --release -p lightshade --bin lightshade
fi
mkdir -p "$WORK"

"$CLI" run "$WORKLOAD" "$DATABASE" insert_message \
    id=1 inbox_id=7 sent_at=100 is_read=false body=restore-drill
"$CLI" backup "$WORKLOAD" "$DATABASE" "$BACKUP"
"$CLI" verify "$WORKLOAD" "$BACKUP"

(
    cd "$WORK"
    sha256sum backup.lsdb > backup.lsdb.sha256
    sha256sum -c backup.lsdb.sha256
)

test "$(stat -c '%a' "$BACKUP")" = "600"

mv "$DATABASE" "$DATABASE.lost"
rm -f "$DATABASE.wal" "$DATABASE.lock"

(
    cd "$WORK"
    sha256sum -c backup.lsdb.sha256
)
install -m 0600 "$BACKUP" "$DATABASE"
"$CLI" verify "$WORKLOAD" "$DATABASE"
"$CLI" run "$WORKLOAD" "$DATABASE" recent_messages inbox=7 limit=10 |
    grep -q 'restore-drill'

"$CLI" run "$WORKLOAD" "$DATABASE" insert_message \
    id=2 inbox_id=7 sent_at=200 is_read=false body=after-restore
printf 'torn' >>"$DATABASE.wal"
"$CLI" recover "$WORKLOAD" "$DATABASE"
"$CLI" verify "$WORKLOAD" "$DATABASE"
"$CLI" run "$WORKLOAD" "$DATABASE" recent_messages inbox=7 limit=10 |
    grep -q 'after-restore'

printf 'snapshot damage' >>"$DATABASE"
if "$CLI" verify "$WORKLOAD" "$DATABASE" >/dev/null 2>&1; then
    echo "verify accepted a damaged snapshot" >&2
    exit 1
fi
install -m 0600 "$BACKUP" "$DATABASE"
"$CLI" verify "$WORKLOAD" "$DATABASE"

echo "restore drill passed: checksum, restore, WAL recovery, and snapshot rejection"