#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SECONDS_TO_RUN="${LIGHTSHADE_SOAK_SECONDS:-60}"
READERS="${LIGHTSHADE_SOAK_READERS:-4}"
WORK="${LIGHTSHADE_SOAK_DIR:-$(mktemp -d)}"
KEEP="${LIGHTSHADE_SOAK_KEEP:-0}"
CLI="${LIGHTSHADE_SOAK_CLI:-$ROOT/target/release/lightshade}"
INTERFACE="$WORK/interface"
DATABASE="$WORK/soak.lsdb"
SOCKET="$WORK/soak.lsdb.sock"
CREDENTIALS="$WORK/credentials"
SECRET="000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
SERVER_PID=""
CLIENTS=()

cleanup() {
    for pid in "${CLIENTS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    for pid in "${CLIENTS[@]}"; do
        wait "$pid" 2>/dev/null || true
    done
    if [[ "$KEEP" != "1" ]]; then
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

if (( SECONDS_TO_RUN < 1 )); then
    echo "LIGHTSHADE_SOAK_SECONDS must be at least 1" >&2
    exit 2
fi
if (( READERS < 1 )); then
    echo "LIGHTSHADE_SOAK_READERS must be at least 1" >&2
    exit 2
fi
if [[ ! -x "$CLI" ]]; then
    cargo build --release -p lightshade --bin lightshade
fi

mkdir -p "$WORK"
"$CLI" build examples/messages.lightshade --out "$INTERFACE"

HASH="$(printf '%s' "$SECRET" | sha256sum | cut -d' ' -f1)"
umask 077
printf 'writer\twriter\t%s\nreader\treader\t%s\n' "$HASH" "$HASH" > "$CREDENTIALS"
chmod 600 "$CREDENTIALS"

echo "soak: ${SECONDS_TO_RUN}s, ${READERS} readers, workdir $WORK"

LIGHTSHADE_CREDENTIALS="$CREDENTIALS" \
LIGHTSHADE_CREDENTIAL_ID=writer \
LIGHTSHADE_CREDENTIAL_SECRET="$SECRET" \
LIGHTSHADE_GROUP_COMMIT_US=1000 \
LIGHTSHADE_CHECKPOINT_BYTES=1048576 \
LIGHTSHADE_LOCK_TIMEOUT_MS=30000 \
LIGHTSHADE_MAX_CONNECTIONS=64 \
LIGHTSHADE_AUDIT_LOG="$WORK/audit.jsonl" \
    "$INTERFACE" "$DATABASE" serve >"$WORK/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 600); do
    [[ -S "$SOCKET" ]] && break
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "soak server exited before opening its socket" >&2
        cat "$WORK/server.log" >&2
        exit 1
    fi
    sleep 0.05
done
if [[ ! -S "$SOCKET" ]]; then
    echo "soak server did not open its socket" >&2
    exit 1
fi

run_client() {
    LIGHTSHADE_CREDENTIALS="$CREDENTIALS" \
    LIGHTSHADE_CREDENTIAL_ID="$1" \
    LIGHTSHADE_CREDENTIAL_SECRET="$SECRET" \
        "$INTERFACE" "$SOCKET" run "${@:2}"
}

: >"$WORK/attempted"
: >"$WORK/acked"
(
    next=0
    while [[ ! -e "$WORK/stop" ]]; do
        next=$((next + 1))
        printf '%s\n' "$next" >>"$WORK/attempted"
        if run_client writer insert_message \
            "id=$next" "inbox_id=$((next % 16))" "sent_at=$next" \
            is_read=false "body=soak-$next" >/dev/null 2>>"$WORK/client.log"; then
            printf '%s\n' "$next" >>"$WORK/acked"
        else
            printf 'writer failed at %s\n' "$next" >>"$WORK/client.log"
            touch "$WORK/writer_failed"
            exit 1
        fi
    done
) &
CLIENTS+=("$!")

for reader in $(seq 1 "$READERS"); do
    (
        while [[ ! -e "$WORK/stop" ]]; do
            run_client reader recent_messages "inbox=$reader" limit=20 \
                >/dev/null 2>>"$WORK/client.log" || {
                    touch "$WORK/reader_failed"
                    exit 1
                }
            run_client reader unread_count "inbox=$reader" \
                >/dev/null 2>>"$WORK/client.log" || {
                    touch "$WORK/reader_failed"
                    exit 1
                }
        done
    ) &
    CLIENTS+=("$!")
done

sleep "$SECONDS_TO_RUN"
touch "$WORK/stop"

for _ in $(seq 1 100); do
    alive=0
    for pid in "${CLIENTS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            alive=1
        fi
    done
    (( alive == 0 )) && break
    sleep 0.05
done
for pid in "${CLIENTS[@]}"; do
    kill "$pid" 2>/dev/null || true
done
for pid in "${CLIENTS[@]}"; do
    wait "$pid" 2>/dev/null || true
done
CLIENTS=()
if [[ -e "$WORK/writer_failed" || -e "$WORK/reader_failed" ]]; then
    echo "soak client failed; see $WORK/client.log" >&2
    cat "$WORK/client.log" >&2
    exit 1
fi

kill -9 "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
SERVER_PID=""

acked="$(wc -l <"$WORK/acked")"
attempted="$(wc -l <"$WORK/attempted")"
if (( acked < 1 )); then
    echo "soak produced no acknowledged writes" >&2
    exit 1
fi

"$INTERFACE" "$DATABASE" verify >"$WORK/verify.log" 2>&1
rows="$("$INTERFACE" "$DATABASE" inspect | awk -F': ' '/^rows:/{print $2}')"
if [[ -z "$rows" ]]; then
    echo "soak inspect did not report a row count" >&2
    exit 1
fi
if (( rows < acked )); then
    echo "soak lost acknowledged writes: rows=$rows acknowledged=$acked" >&2
    exit 1
fi
if (( rows > attempted )); then
    echo "soak retained an unattempted write: rows=$rows attempted=$attempted" >&2
    exit 1
fi
if ! grep -q '"event":"auth_success"' "$WORK/audit.jsonl"; then
    echo "soak audit log is missing auth events" >&2
    exit 1
fi

echo "soak passed: attempted=$attempted acknowledged=$acked recovered_rows=$rows"