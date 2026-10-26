# Storage format

One database is a file plus sidecars: `<name>.lsdb` (snapshot),
`<name>.lsdb.wal`, `<name>.lsdb.lock`, `<name>.lsdb.sock` (the host
endpoint file). Everything the engine writes is owner-only (`0600`),
behind a named constant so macro-generated code can't trip the
non-octal-permissions lint.

## Snapshot

Magic `LIGHTSS1`, format version **`8`**. Versions **7** and **6**
stay readable (6 maps old snapshots by stored name). Rows carry
presence bytes for nullable columns; the schema fingerprint only
bends when nullability does. Chunked at 1 MiB per write with a
512 MiB chunk cap; big tables segment across chunks. `compact`
rewrites the snapshot and truncates the WAL.

## WAL

40-byte header, 256 MiB record cap, version **`3`**. Two legacy
layouts replay: version **5** (old *operation* records) and version
**2** (old *effect* records), through separate replay branches:
operations versus effects are different shapes, not interchangeable
history. Every record carries the schema fingerprint, so a WAL from
a different schema generation is rejected before replay rather than
half-applied. Group commit batches fsyncs inside the window;
automatic checkpoints preserve only acknowledged writes.

## Version table

| Component | Current | Also readable | Otherwise |
|---|---|---|---|
| Snapshot | 8 | 7, 6 | `UnsupportedVersion` |
| WAL | 3 | 5 (operations), 2 (effects) | `UnsupportedVersion` |
| Fingerprint | order-sensitive schema hash | none | `SchemaMismatch` (migrate, don't repair) |

Note the fingerprint row: field *order* matters, so reordered fields
are a migration, not a no-op. That surprises people exactly once.

## Crash behavior

A torn tail truncates to the last committed prefix: acknowledged
writes survive, unacknowledged tail bytes don't. A WAL damaged
beyond the tail is discarded only with `--discard-wal`, reopening at
the last checkpoint. The old WAL is quarantined aside first,
deleted on success, restored on failure. Snapshot damage (bad magic,
checksum mismatch at an LSN, truncation, moved-during-read) is never
repaired in place. Restore from backup. `verify` tells you which of
these you're dealing with before you act, which is the entire point
of running it first.
