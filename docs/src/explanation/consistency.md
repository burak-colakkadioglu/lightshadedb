# Consistency and recovery

## Transactions

`Backend::transaction` runs a closure atomically: `Ok` commits,
`Err` rolls back. Use the closure form unless you have a reason not
to. It makes the atomic boundary visible and unmissable. Explicit
`begin` / `commit` / `rollback` cover the shapes closures can't.
The async `Transaction` handle consumes itself on commit or rollback
and poisons its connection on drop: abandoning one rolls back
rather than leaking a half-applied write or a held lock.

Isolation is prefix-based, not lock-y in spirit: concurrent readers
each see a consistent committed prefix, writers serialize per table,
and a write conflict under a hosted transaction fails the whole
thing with `Conflict { table }` instead of interleaving partial
effects. No phantom reads, no half-written rows, no "it worked on
my machine" isolation levels to configure.

## Durability

Every acknowledged write is fsync'd (possibly batched inside the
group-commit window) before its call returns. Checkpoints preserve
only acknowledged writes: after a crash, the database holds a prefix
of the acknowledged sequence: never a suffix with holes, never a
torn middle. `automatic_checkpoints_preserve_acknowledged_writes`
pins this, and the soak suite hammers it for over a minute of
concurrent readers and writers with tiny checkpoints and the audit
log on.

## Recovery order

1. Replay the snapshot.
2. Validate the WAL header and fingerprint; reject foreign WALs
   before replay instead of half-applying them.
3. Truncate a torn tail to the last committed prefix.
4. Replay records; adopt effects into live state.

Readers open across the crash are unaffected. Each sees the prefix
that was committed when its snapshot was taken. The fault-injection
suite breaks this on purpose with an `LD_PRELOAD` shim that fails
writes, syncs, and renames at scripted points
(`write_failures_surface_as_errors…`, `crash_recovery_keeps…`,
`fault_injection_preserves…`). If recovery ever gets cleverer than
"truncate to prefix, replay forward," those tests will be the first
to complain.

## What is *not* guaranteed

- Unacknowledged writes (returned but not fsync'd before the crash)
  may be lost. The group-commit window trades latency for exactly
  this exposure. Size it deliberately.
- Anything after the last checkpoint is gone when the WAL is
  discarded. Size checkpoints against the backup window, not against
  your optimism.
- CSV import and dump restore are strict and non-transactional: a
  malformed input fails the whole operation instead of applying a
  prefix. Half-applied bulk loads are worse than failed ones.
