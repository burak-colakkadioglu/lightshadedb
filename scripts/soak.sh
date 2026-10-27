#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SECONDS_TO_RUN="${LIGHTSHADE_SOAK_SECONDS:-60}"
READERS="${LIGHTSHADE_SOAK_READERS:-4}"
WRITERS="${LIGHTSHADE_SOAK_WRITERS:-1}"
WORK="${LIGHTSHADE_SOAK_DIR:-$(mktemp -d)}"
KEEP="${LIGHTSHADE_SOAK_KEEP:-0}"
CLI="${LIGHTSHADE_SOAK_CLI:-$ROOT/target/release/lightshade}"
INTERFACE="$WORK/interface"
DATABASE="$WORK/soak.lsdb"
SOCKET="$WORK/soak.lsdb.sock"
CREDENTIALS="$WORK/credentials"
SECRET="000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
SOAK_LOGS="${LIGHTSHADE_SOAK_LOGS:-$ROOT/soak-logs}"
SERVER_PID=""
CLIENTS=()

archive_logs() {
    mkdir -p "$SOAK_LOGS" 2>/dev/null || return 0
    for log in server.log client.log audit.jsonl verify.log; do
        [[ -e "$WORK/$log" ]] && cp "$WORK/$log" "$SOAK_LOGS/" 2>/dev/null || true
    done
}

cleanup() {
    for pid in ${CLIENTS[@]+"${CLIENTS[@]}"}; do
        kill "$pid" 2>/dev/null || true
    done
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    for pid in ${CLIENTS[@]+"${CLIENTS[@]}"}; do
        wait "$pid" 2>/dev/null || true
    done
    if [[ "$KEEP" != "1" ]]; then
        rm -rf "$WORK"
    fi
}

finish() {
    local code=$?
    if (( code != 0 )); then
        archive_logs
    fi
    cleanup
}
trap finish EXIT

if (( SECONDS_TO_RUN < 1 )); then
    echo "LIGHTSHADE_SOAK_SECONDS must be at least 1" >&2
    exit 2
fi
if (( READERS < 1 )); then
    echo "LIGHTSHADE_SOAK_READERS must be at least 1" >&2
    exit 2
fi
if (( WRITERS < 1 )); then
    echo "LIGHTSHADE_SOAK_WRITERS must be at least 1" >&2
    exit 2
fi
if [[ ! -x "$CLI" ]]; then
    cargo build --release -p lightshadedb --bin lightshade
fi

mkdir -p "$WORK"
"$CLI" build examples/messages.lightshade --out "$INTERFACE"

HASH="$(printf '%s' "$SECRET" | sha256sum | cut -d' ' -f1)"
umask 077
printf 'writer\twriter\t%s\nreader\treader\t%s\n' "$HASH" "$HASH" > "$CREDENTIALS"
chmod 600 "$CREDENTIALS"

echo "soak: ${SECONDS_TO_RUN}s, ${WRITERS} writers, ${READERS} readers, workdir $WORK"

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

probe_ready() {
    LIGHTSHADE_CREDENTIALS="$CREDENTIALS" \
    LIGHTSHADE_CREDENTIAL_ID=reader \
    LIGHTSHADE_CREDENTIAL_SECRET="$SECRET" \
        "$INTERFACE" "$SOCKET" run unread_count inbox=0 >/dev/null 2>&1
}

for _ in $(seq 1 600); do
    if probe_ready; then
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "soak server exited before opening its socket" >&2
        cat "$WORK/server.log" >&2
        exit 1
    fi
    sleep 0.05
done
if ! probe_ready; then
    echo "soak server did not open its socket" >&2
    cat "$WORK/server.log" >&2 || true
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
for writer in $(seq 1 "$WRITERS"); do
    (
        base=$((writer * 1000000000))
        next=0
        while [[ ! -e "$WORK/stop" ]]; do
            next=$((next + 1))
            id=$((base + next))
            printf '%s\n' "$id" >>"$WORK/attempted"
            if run_client writer insert_message \
                "id=$id" "inbox_id=$((id % 16))" "sent_at=$id" \
                is_read=false "body=soak-$id" >/dev/null 2>>"$WORK/client.log"; then
                printf '%s\n' "$id" >>"$WORK/acked"
            else
                printf 'writer failed at %s\n' "$id" >>"$WORK/client.log"
                touch "$WORK/writer_failed"
                exit 1
            fi
        done
    ) &
    CLIENTS+=("$!")
done

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
    for pid in ${CLIENTS[@]+"${CLIENTS[@]}"}; do
        if kill -0 "$pid" 2>/dev/null; then
            alive=1
        fi
    done
    (( alive == 0 )) && break
    sleep 0.05
done
for pid in ${CLIENTS[@]+"${CLIENTS[@]}"}; do
    kill "$pid" 2>/dev/null || true
done
for pid in ${CLIENTS[@]+"${CLIENTS[@]}"}; do
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