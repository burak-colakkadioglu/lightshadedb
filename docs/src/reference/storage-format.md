# Storage format

One database is a file plus sidecars: `<name>.lsdb` (snapshot),
`<name>.lsdb.wal`, `<name>.lsdb.lock`, `<name>.lsdb.sock` (the host
endpoint file). Everything the engine writes is owner-only (`0600`),
behind a named constant so macro-generated code can't trip the
non-octal-permissions lint.

## Snapshot

Magic `LIGHTSS1`, format version **`9`**. Versions **8**, **7** and **6**
stay readable (6 maps old snapshots by stored name). Rows carry
presence bytes for nullable columns; the schema fingerprint only
bends when nullability does. Chunked at 1 MiB per write with a
512 MiB chunk cap; big tables segment across chunks. `compact`
rewrites the snapshot and truncates the WAL. Version 9 chunks may
carry compression or encryption: the length word reserves the top
two bits as codec flags, the stored bytes hold the transformed
payload, and the recorded hash covers the original chunk bytes.
Readers on version 9 still replay versions 8, 7 and 6 as plain
chunks with no flags.

## WAL

40-byte header, 256 MiB record cap, version **`4`**. Two legacy
layouts replay: version **5** (old *operation* records) and version
**2** (old *effect* records), plus version **3** (previous *effect*
records), through separate replay branches:
operations versus effects are different shapes, not interchangeable
history. Every record carries the schema fingerprint, so a WAL from
a different schema generation is rejected before replay rather than
half-applied. Group commit batches fsyncs inside the window;
automatic checkpoints preserve only acknowledged writes. Version 4
records reuse the 40-byte header and reserve the top two bits of the
length word as codec flags; the checksum still covers the original
record bytes so a wrong key or a damaged payload fails the same
checksum comparison. An all-zero header or an all-zero tail is
treated as preallocated space and truncates to the last committed
prefix, the same path torn writes already use.

## Preallocation

The WAL file is pre-grown in fixed 8 MiB segments with `set_len`
(plain std, no `fallocate`). Open truncates the torn tail to the
last valid offset and then grows to valid plus one segment.
Each batch grows again while fewer than 1 MiB of preallocated
space remain. `LIGHTSHADE_WAL_PREALLOC_BYTES` overrides the segment
size in bytes; `0` disables preallocation. Preallocated zeros never
replay as records.

## Compression

Optional, off by default. Set `LIGHTSHADE_COMPRESSION` to `1`,
`true`, `yes`, `on`, `rle` or `lz` to compress snapshot chunks and
WAL records with the built-in byte codec. The encoder stores the
compressed form only when it is smaller than the original; small or
incompressible payloads are stored as-is. Old snapshots and WAL records
without flags stay readable with compression on or off, and new
flagged files are rejected by older readers as unsupported versions.

## Encryption

Optional, off by default. Set `LIGHTSHADE_ENCRYPTION_KEY_FILE` to a
private key file holding 32 raw bytes or 64 hex characters. The
file must be a regular file (not a symlink) and must not be
readable by group or others; violations refuse to open with
`InvalidData`. When set, snapshot chunks and WAL records are sealed
with an AEAD built from SHA-256 in counter mode plus a SHA-256
Encrypt-then-MAC tag, with a fresh 12-byte nonce per record or
chunk. Key bytes are zeroized on drop. Opening with the wrong key
or with no key fails as `Corruption` (an `InvalidData` checksum or
authentication error), never a panic, and leaves the files
untouched.

## Version table

| Component | Current | Also readable | Otherwise |
|---|---|---|---|
| Snapshot | 9 | 8, 7, 6 | `UnsupportedVersion` |
| WAL | 4 | 3 (effects), 5 (operations), 2 (effects) | `UnsupportedVersion` |
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
