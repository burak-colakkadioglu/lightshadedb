# Troubleshooting

Errors here are typed, so start by reading the type. Three questions,
in order, solve most incidents.

## 1. Usage or runtime?

The CLI exits `2` for usage errors (unknown command, wrong arity,
bad flags — anything starting with `usage:`) and `1` for runtime
failures. Branch scripts on the code, not on stderr text. But note:
library errors carry their own codes — `Corruption` exits 3,
`Conflict` 4, `Auth` 5, `Quota` 6, `Unsupported` 7, `Migration` 8,
everything else 1. The `2` belongs to CLI argument handling, not to
`Error::exit_code()`.

## 2. Retryable or not?

`Error::is_retryable()` plus `retry_after()` is the *entire* retry
policy, and it's narrower than you'd guess: only `Conflict` (retry
immediately, no delay) and `Busy` (wait 50 ms) are retryable.
`Quota` errors are never retryable — even though they carry a
one-second `retry_after` hint, retrying an over-limit write cannot
succeed. Timeouts are never retried. Transactions surface errors
instead of retrying. If your retry loop treats every error the same,
this is the section that saves you.

## 3. Is data at risk?

Only `Corruption` variants imply possible data loss. Everything else
is safe to retry, reconfigure, or escalate.

## The greatest hits

**`database is locked`** — a second writer holds the lock, or a
crashed process left a stale one. If nothing is alive, remove the
`.lock` file next to the database and reopen. The wait is bounded by
`LIGHTSHADE_LOCK_TIMEOUT_MS`.

**`stored schema does not match this build`** — this is a migration,
not corruption. The fingerprint gate compared your compiled schema
against the stored one and they differ. Run `migrate --dry-run`,
read the plan, apply it. [Evolving a schema](../tutorials/evolving-a-schema.md)
walks the whole flow.

**`authentication failed`** — deliberately vague. It covers bad
credentials, expired credentials, *and* unknown database names, so
attackers can't probe for database names. Check the credential id,
the secret, and the endpoint's database segment, in that order.

**`feature X is not enabled`** — the binary was built without the
Cargo feature the operation needs. Rebuild with the feature; the
[feature matrix](../reference/deployment-modes.md#features) lists
every supported combination.

**Compile errors naming fields** (`must have exactly one primary
key`, `unknown field`, `nullable join`) come from the schema macro,
not the database. They name the table, the field, and the rule. Fix
the schema and rebuild — the compiler just did you a favor.

## Crash checklist

1. `verify` the database. Snapshot damage means restore-from-backup;
   WAL damage means recovery truncates to the committed prefix
   automatically.
2. If the WAL is unusable beyond the tail, `recover --discard-wal`
   reopens at the last checkpoint. Accept the loss window — your
   drill already taught you its size.
3. Run the restore drill against the recovered file before returning
   it to service. Recovered is not the same as verified.

## Filing a useful report

Four things: the schema diff (if any), the exact command and its
exit code, the full error text, and whether `verify` passes. Reports
without `verify` output go to the back of the queue — one command
distinguishes "my query is wrong" from "my data is damaged", and
we're going to ask for it anyway.
