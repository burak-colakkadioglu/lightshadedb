# Quotas and limits

Quotas are enforced counters, not suggestions. When one trips, the
operation fails with a typed `Quota` error naming the kind and the
limit. Nothing is silently dropped, nothing half-applies.

## Quota kinds

| `QuotaKind` | Counts | Trips when |
|---|---|---|
| `Rows` | rows per table | bulk inserts, imports, restores outgrow the cap |
| `WalBytes` | un-checkpointed WAL | write bursts outrun checkpoints |
| `Connections` | concurrent server connections | connection storms (default cap: 128) |
| `Transactions` | concurrent transactions | overlapping writers (default cap: 128) |

Now the part that surprises people: quota errors are **never
retryable**, even though they carry a `retry_after` hint (one
second). Retrying an over-limit write cannot succeed — fix the
request or raise the limit. The only retryable errors are `Conflict`
(no delay — retry immediately) and `Busy` (50 ms hint). The pool
knows this: it sleeps only when a hint exists, retries only what
reports retryable, and never retries timeouts at all.

## Tuning throughput

Two knobs shape the write path, and they're separate things people
conflate:

- **Group commit** batches fsyncs inside a microsecond window.
  Generated servers default to 1 ms; the legacy CLI path reads
  `LIGHTSHADE_GROUP_COMMIT_US` (default 1000 µs, capped at a second).
  The soak script runs at exactly the default — it's not special
  tuning, it's the steady state. Raise the window when `verify` is
  healthy but throughput plateaus on fsync latency.
- **Checkpoints** fire at `LIGHTSHADE_CHECKPOINT_BYTES` (default
  8 MiB; `0` disables automatic checkpoints entirely). Lower it when
  WAL growth outpaces your backup window; raise it when checkpoints
  themselves become the bottleneck.

Row cache (`LIGHTSHADE_ROW_CACHE_BYTES`, 64 MiB) and the row/WAL caps
(`LIGHTSHADE_MAX_ROWS`, `LIGHTSHADE_MAX_WAL_BYTES`, both `0` for
unlimited) complete the memory picture. See [Environment](reference/environment.md)
for the full table with defaults.

## Spill and audit

Large scans spill to `LIGHTSHADE_SPILL_DIR` instead of growing the
heap without bound. Unset means the system temp dir; explicitly set
means that directory must exist, or startup fails rather than
silently running unbounded. Spill files are owner-only and cleaned up
on normal shutdown; after a crash, stale spill is ignored, never
replayed.

`LIGHTSHADE_AUDIT_LOG` appends one JSON object per connection
lifecycle event. It's observability, not a WAL — never parse it for
recovery, and rotate it like any other log. Slow-query sampling
(`LIGHTSHADE_SLOW_LOG`, threshold `LIGHTSHADE_SLOW_MS`, default
100 ms) records operation, strategy, timing, and rows — never
parameters.
