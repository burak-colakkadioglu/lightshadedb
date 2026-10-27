# Wire protocol (LSRP v6)

LSRP connects generated clients to generated servers over TCP or Unix
sockets. Version 5 adds the requested database name to the handshake,
so one listener can serve many databases without one handshake per
tenant. Version 6 adds two requests for follower tailing (`WalMeta`,
`WalChunk`); servers accept both 5 and 6, and plain clients never
need 6.

## Framing

Magic `LSRP`, version `u32` (currently `5`). Every frame is
length-prefixed (`u32` little-endian) with a 256 MiB cap
(`MAX_FRAME`); handshake payloads cap at 64 KiB
(`MAX_HANDSHAKE`). An oversized length is a `Protocol` error; a
short read is an I/O error. The distinction matters because only
the former says "the peer is speaking nonsense" as opposed to "the
network died." Either way, that connection is done. Malformed frames
never take down the server, only the offending connection.

## Handshake

1. Client sends magic, version, workload fingerprint (`u64`),
   database name, credential id, and secret.
2. Server checks magic, version, and fingerprint, looks up the
   credential, and compares secrets in constant time.
3. Success is status `0`; failure is the same `1`-plus-error-body
   framing as data responses.

Unknown database names fail identically to bad credentials
(`AuthError::Failed`). No oracle, no probing, no special cases.

## Requests

One request, one response. Kinds are tagged `0..=6`: `Operation`
(carries index plus typed parameters), `Begin`, `Commit`,
`Rollback`, `Checkpoint` (no parameters: server runs a checkpoint
including spill vacuum and answers empty), `WalMeta` (no parameters: primary answers schema
fingerprint, LSN, and valid WAL length in bytes), `WalChunk` (takes u64 offset and
length, answers the raw WAL bytes). Non-operation requests carry no
index; `WalMeta` and `WalChunk` carry parameters. The encoder omits
them otherwise. Operation indexes are positions in the
workload's operation table, not names: renaming an operation without
rebuilding both sides breaks the pairing, which is why client and
server builds pin the same workload fingerprint. Follower tailing
requires version 6 on both sides; a version 5 peer gets a clear
`Protocol` error instead of a silent mismatch.

## Values

Fourteen typed values plus typed `NULL` (tag 14 + declared type tag,
no payload). Texts and byte strings are length-prefixed; UTF-8 is
validated on decode. Rows encode as a `u32` count followed by flat
values; parameter lists cap at a million entries. Short reads,
unknown tags, and bad UTF-8 all decode to `Protocol` errors:
never panics, never silent truncation.

## Errors on the wire

Status `0` plus values, or nonzero plus a coded body: `ErrorCode`,
optional `RemoteDetail`, message. Known remote errors reconstruct
the corresponding local variants on the client, so `match` code
written against embedded errors works unchanged over the network;
unknown codes and unknown detail tags degrade to `Protocol` errors
naming the code. There is a wrinkle worth knowing: detail-less auth
errors and a few others reconstruct with a `"<remote>"` table
placeholder (`reconstruct_remote`), because the wire doesn't always
carry what the local variant wants.

## Timeouts and retries

`PoolOptions` defaults to a 5-second connect and 30-second I/O
timeout. The retry policy lives client-side and is deliberately
narrow: pooled single operations retry errors reporting
`is_retryable()`, sleeping `retry_after()` when present, up to
`max_retries`. Timeouts are never retried. Transactions surface
errors instead of retrying. Replaying a transaction's prefix
without its context would be worse than failing.
