# Environment variables

Everything configurable without recompiling, in one place. `0`
means unlimited where limits are concerned; time durations have per-variable
units, so read the unit column, not your instincts.

## Engine

| Variable | Default | Meaning |
|---|---|---|
| `LIGHTSHADE_CHECKPOINT_BYTES` | 8 MiB | WAL bytes before an automatic checkpoint; `0` disables auto-checkpoints |
| `LIGHTSHADE_ROW_CACHE_BYTES` | 64 MiB | read cache budget |
| `LIGHTSHADE_MAX_ROWS` | `0` (unlimited) | per-table row cap → `QuotaKind::Rows` |
| `LIGHTSHADE_MAX_WAL_BYTES` | `0` (unlimited) | un-checkpointed WAL cap → `QuotaKind::WalBytes` |
| `LIGHTSHADE_SPILL_DIR` | system temp dir | spill directory; must exist if set, or startup fails |
| `LIGHTSHADE_LOCK_TIMEOUT_MS` | engine default | how long a second writer waits for the lock |

## Server

| Variable | Default | Meaning |
|---|---|---|
| `LIGHTSHADE_CREDENTIALS` | required | credentials file path (`id secret-hash role` rows) |
| `LIGHTSHADE_CREDENTIAL_ID` / `_SECRET` | required (client) | client identity and secret |
| `LIGHTSHADE_LISTEN` | derived socket | host bind address; unset means `<db>.lsdb.sock`, `0600` |
| `LIGHTSHADE_NETWORK_ACCESS` | inferred | `local` / `local-network` / `external`; wildcard binds require explicit opt-in |
| `LIGHTSHADE_MAX_CONNECTIONS` | 128 | concurrent connection cap → `QuotaKind::Connections` |
| `LIGHTSHADE_MAX_TRANSACTIONS` | 128 | concurrent transaction cap → `QuotaKind::Transactions` |
| `LIGHTSHADE_GROUP_COMMIT_US` | 1000 µs | fsync batching window |
| `LIGHTSHADE_FULL_SYNC` | off | macOS only: use full `F_FULLFSYNC` hardware flushes instead of barrier syncs (which fall back to plain `fsync` where unsupported) |
| `LIGHTSHADE_IDLE_TIMEOUT_MS` | 30 s | idle connection timeout |
| `LIGHTSHADE_HANDSHAKE_TIMEOUT_MS` | 10 s | handshake deadline |
| `LIGHTSHADE_SHUTDOWN_TIMEOUT_MS` | 10 s | drain deadline on shutdown |
| `LIGHTSHADE_TENANT` | db stem or `default` | tenant label |
| `LIGHTSHADE_CREDENTIALS_RELOAD_MS` | 1 s | credential file reload interval |
| `LIGHTSHADE_AUDIT_LOG` | off | JSONL connection lifecycle log |

## Observability

| Variable | Default | Meaning |
|---|---|---|
| `LIGHTSHADE_SLOW_LOG` | off | JSONL slow-query log path |
| `LIGHTSHADE_SLOW_MS` | 100 ms | sampling threshold; logs strategy, timing, rows — never parameters |

## Application and migration

| Variable | Default | Meaning |
|---|---|---|
| `LIGHTSHADE_MODE` | `embedded` (or `client`) | `App::from_env` mode; embedded default applies when that feature is on |
| `LIGHTSHADE_PATH` | required in embedded mode | database path for `from_env` |
| `LIGHTSHADE_ENDPOINT` | required in client mode | endpoint for `from_env` |
| `LIGHTSHADE_MIGRATION_BACKUP` | true | back up before `--apply` |
| `LIGHTSHADE_ALLOW_DESTRUCTIVE_MIGRATION` | false | allow destructive steps |
| `LIGHTSHADE_<ID>_PATH` et al. | — | per-registry-entry overrides ([Registry](registry.md)) |

## CLI internals

| Variable | Meaning |
|---|---|
| `LIGHTSHADE_INTERFACE` | override the cached interface binary path |
| `LIGHTSHADE_CACHE` | override the interface cache dir (default: platform cache dir) |

`RUSTC` is honored when building interface binaries. None of these
are API; they're listed so that when a script sets one, you know
where to look.
