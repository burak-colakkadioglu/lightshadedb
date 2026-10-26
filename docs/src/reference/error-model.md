# Error model

Every fallible operation returns `Result<T, Error>`. Errors are
typed, stable to match on, and identical in shape for embedded and
connected use. Remote failures reconstruct the local variants, so
your `match` doesn't care which side of a socket it's on.

## The `Error` enum

| Variant | Meaning |
|---|---|
| `Io { context, source }` | OS failure with a static context string |
| `Corrupt(Corruption)` | snapshot or WAL failed validation |
| `Schema(SchemaError)` | compiled schema invalid or incompatible |
| `Quota { kind, limit }` | a configured quota tripped |
| `Conflict { table }` | write conflict under a hosted transaction; rolled back |
| `DuplicateKey { table }` | primary key already exists |
| `ReadOnly` | handle or server refuses writes |
| `Locked { path }` | another process holds the writer lock |
| `Busy` | transient contention, retryable, 50 ms hint |
| `NotFound { path }` | missing file or endpoint |
| `Auth(AuthError)` | `Failed`, `Expired`, or `PermissionDenied` |
| `Protocol { reason }` | wire or encoding violation |
| `Unsupported { feature }` | binary lacks the Cargo feature |
| `InvalidArgument { name, reason }` | bad endpoint, CIDR, flag, or parameter |
| `Failed { reason }` | handle-level failure |
| `Migration(MigrationError)` | migration planning or application failure |
| `Remote(Box<RemoteError>)` | reconstructed server-side error |
| `Internal { reason }` | a bug: report it with a reproduction |

`Corruption` enumerates bad magic, unsupported versions, checksum
mismatch at an LSN, torn WAL tail, truncation, invalid encoding,
snapshot-moved-during-read, schema mismatch, and table mismatch.
`SchemaError` is the longer list people underestimate: duplicates,
missing or multiple primary keys, primary-key defaults, type
mismatches, missing defaults, unsupported defaults, legacy WAL
markers, bad renames, unknown tables *and* unknown fields.

## The policy methods

- `kind() -> ErrorKind`: coarse category for metrics and alerting.
- `code() -> ErrorCode`: stable `u16` for the wire (`1..=18`,
  `Remote = 16`, `Internal = 17`, `Migration = 18`); round-trips,
  and `0` or `19+` from the wire degrades to `Protocol`.
- `is_retryable()` + `retry_after()`: the whole retry policy. It's
  asymmetric on purpose: `Conflict` is retryable with no delay,
  `Busy` is retryable with a 50 ms hint, and `Quota` is *never*
  retryable despite carrying a one-second hint (retrying an
  over-limit write cannot succeed). The pool sleeps only when a hint
  exists and retries only what reports retryable.
- `exit_code()`: `3` corruption, `4` conflict, `5` auth, `6` quota,
  `7` unsupported, `8` migration, `1` everything else. Note what's
  missing: `0` and `2` are CLI process codes for success and usage
  errors, not `Error` codes. Conflating them breaks script branching.
- `remote_detail()`: structured conflict / duplicate / quota /
  schema / auth detail for programmatic handling. Only some schema
  errors (`UnknownField`, `UnknownTable`) and `Auth(Failed)` carry
  detail; the rest return `None`, so don't assume its presence.

One rule for all of it: match on variants, never on message strings.
Messages are human text and may change; variants and codes are API,
and `#[non_exhaustive]` means new variants can arrive without a
major version. Match defensively with a fallback arm.
