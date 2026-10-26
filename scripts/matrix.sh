#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RUN_TESTS=0
if [[ "${1:-}" == "--tests" ]]; then
    RUN_TESTS=1
fi

pass=0
fail=0

check() {
    local label="$1"
    shift
    if "$@" >/tmp/lightshade-matrix.log 2>&1; then
        pass=$((pass + 1))
        echo "ok   $label"
    else
        fail=$((fail + 1))
        echo "FAIL $label (see /tmp/lightshade-matrix.log)"
    fi
}

echo "== facade feature combinations =="
check "lightshade/no-default"        cargo check -q -p lightshade --no-default-features
check "lightshade/client-only"        cargo check -q -p lightshade --no-default-features --features client
check "lightshade/embedded"           cargo check -q -p lightshade --no-default-features --features embedded
check "lightshade/host"               cargo check -q -p lightshade --no-default-features --features host
check "lightshade/host+client"        cargo check -q -p lightshade --no-default-features --features host,client
check "lightshade/all-features"       cargo check -q -p lightshade --all-features

echo "== application feature combinations =="
check "rust-messages/embedded"        cargo check -q -p rust-messages
check "rust-messages/client-only"     cargo check -q -p rust-messages --no-default-features --features client
check "rust-messages/host"            cargo check -q -p rust-messages --features host
check "rust-messages/host+client"     cargo check -q -p rust-messages --features host,client
check "rust-messages/async"           cargo check -q -p rust-messages --no-default-features --features host,client,async
check "rust-suite/embedded"           cargo check -q -p rust-suite
check "rust-suite/host+client"        cargo check -q -p rust-suite --features host,client
check "lightshade/bin"                cargo check -q -p lightshade --bin lightshade

if [[ "$RUN_TESTS" == "1" ]]; then
    echo "== mode test suites =="
    check "test:embedded"             cargo test -q -p rust-messages
    check "test:client-only"          cargo test -q -p rust-messages --no-default-features --features client
    check "test:host+client"          cargo test -q -p rust-messages --features host,client
    check "test:async"                cargo test -q -p rust-messages --no-default-features --features host,client,async
    check "test:equivalence"          cargo test -q -p rust-suite
    check "test:facade"               cargo test -q -p lightshade --all-features
fi

echo "== $pass passed, $fail failed =="
[[ "$fail" == "0" ]]