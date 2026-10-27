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
| `LIGHTSHADE_WAL_PREALLOC_BYTES` | 8 MiB | WAL preallocation segment in bytes; `0` disables preallocation |
| `LIGHTSHADE_COMPRESSION` | off | `1`/`true`/`yes`/`on`/`rle`/`lz` compresses snapshot chunks and WAL records with the built-in codec |
| `LIGHTSHADE_RELAXED` | off | `1`/`true`/`yes`/`on` buffers WAL writes without an fsync per commit; call `flush()` or `checkpoint()` for durability, or use `Db::open_relaxed` / `OpenOptions { relaxed: true }` |
| `LIGHTSHADE_ENCRYPTION_KEY_FILE` | off | key file path for encryption at rest (32 raw bytes or 64 hex chars, `0600`, regular file) |

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
| `LIGHTSHADE_SLOW_MS` | 100 ms | sampling threshold; logs strategy, timing, rows. Never parameters |

## Application and migration

| Variable | Default | Meaning |
|---|---|---|
| `LIGHTSHADE_MODE` | `embedded` (or `client`) | `App::from_env` mode; embedded default applies when that feature is on |
| `LIGHTSHADE_PATH` | required in embedded mode | database path for `from_env` |
| `LIGHTSHADE_ENDPOINT` | required in client mode | endpoint (`tcp://`, `unix://`, bare path) or `lightshadedb://` connection URL for `from_env` |
| `LIGHTSHADE_MIGRATION_BACKUP` | true | back up before `--apply` |
| `LIGHTSHADE_ALLOW_DESTRUCTIVE_MIGRATION` | false | allow destructive steps |
| `LIGHTSHADE_<ID>_PATH` et al. | varies | per-registry-entry overrides ([Registry](registry.md)) |

## CLI internals

| Variable | Meaning |
|---|---|
| `LIGHTSHADE_INTERFACE` | override the cached interface binary path |
| `LIGHTSHADE_CACHE` | override the interface cache dir (default: platform cache dir) |

`RUSTC` is honored when building interface binaries. None of these
are API; they're listed so that when a script sets one, you know
where to look.

## Metrics

In-process counters with no extra dependencies. Read them with
`lightshadedb::metrics::snapshot()` for a `Snapshot` of `u64` values
or `lightshadedb::metrics::exposition()` for Prometheus style text
with one `name value` line per counter. Collection is always on.
Tuning happens through the variables above.

| Metric | Related env | Meaning |
|---|---|---|
| `lightshade_connections_total` | `LIGHTSHADE_MAX_CONNECTIONS`, `LIGHTSHADE_IDLE_TIMEOUT_MS`, `LIGHTSHADE_HANDSHAKE_TIMEOUT_MS` | accepted connections since process start |
| `lightshade_group_commit_batches_total` | `LIGHTSHADE_GROUP_COMMIT_US` | group commit fsync batches run by the durability waiter |
| `lightshade_wal_bytes_written_total` | `LIGHTSHADE_CHECKPOINT_BYTES`, `LIGHTSHADE_MAX_WAL_BYTES` | WAL bytes written since process start |
| `lightshade_cache_hits_total` | `LIGHTSHADE_ROW_CACHE_BYTES` | read cache hits |
| `lightshade_cache_misses_total` | `LIGHTSHADE_ROW_CACHE_BYTES` | read cache misses |
| `lightshade_slow_queries_total` | `LIGHTSHADE_SLOW_MS`, `LIGHTSHADE_SLOW_LOG` | queries slower than the sampling threshold |
