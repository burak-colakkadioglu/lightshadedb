# Contributing

The tree stays green at every commit: `cargo fmt --check`, `clippy
--workspace --all-targets -- -D warnings`, the full workspace suite,
and `scripts/matrix.sh --tests` (20 checks: compile configurations
plus embedded, client-only, host+client, async, equivalence, and
facade suites). If any of that is red, the commit doesn't land.
There are no "fix it later" commits.

## How work gets done

Subagents and exploration are read-only recon — they map the
territory, they don't change it. Changes land as small commits with
the gates above passing at each step. Splits and refactors must be
logic-free: byte-identical generated output is the proof
(`keeps_generated_code_deterministic` plus the dataplane
interchange suite pin it), not a feeling about the diff.

`/tmp` is a small tmpfs: clean `lightshade-compile-fail-target-*`,
`lightshade-codegen-*`, and `lightshade-build-*` after test runs.
Never touch `/tmp/opencode/pgdata` or `/tmp/tmp.*`. Somebody's
database lives there.

## Test suites

| Suite | What it pins |
|---|---|
| `lightshadedb` lib + `cli` + `dataplane_compat` | facade behavior, exit codes, CLI↔API byte identity |
| `lightshadedb-codegen` lib (73 tests) | planner, emitters, persistence, recovery, servers |
| `compile_fail` (trybuild, 15 cases) | the important schema diagnostic messages |
| `rust-messages` (embedded/network/nullable/pool/…) | per-mode behavior over the fixture |
| `rust-suite` equivalence | host+client agrees with embedded |

New diagnostics need a compile-fail case. New storage behavior
needs a fault-injection or recovery case. New CLI output needs a
`cli.rs` case, including the exit code. No test, no merge — the
table above is the definition of "covered."

Contributions are dual-licensed MIT OR Apache-2.0 under the root
`CONTRIBUTING` terms, which include a relicensing grant: the project
may relicense contributions later without further approval. Read the
file before your first PR.

## Drills

- `restore-drill.sh` — backup → checksums → simulated loss →
  restore → canary read, then torn-WAL recovery and
  snapshot-damage rejection. Run it after touching backup,
  restore, verify, or file-permission code.
- `soak.sh` — 60+ seconds of concurrent readers and writers with
  group commit, tiny checkpoints, and the audit log on. Run it
  after touching the server, pool, WAL, or checkpoint paths. Slow
  by design and opt-in; it is correctly *not* in the matrix.

## Docs

This book is the documentation, and the code carries no comments by
policy — which means behavior changes must update the relevant page
in the same commit. New operations touch the query-language and
schema-macro pages; new flags touch the CLI reference; new versions
touch the storage-format table; new errors touch the error-model
table; new knobs touch the environment page. The `cli.md` command
table should match `--help` verbatim — if they disagree, the page
is the bug.
