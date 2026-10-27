# Deployment modes

One schema, four Cargo features, one trait. Application code written
against `Backend` compiles and runs in every mode, so deployment
stays a manifest edit instead of a rewrite.

## Features

| Feature | Default | Provides |
|---|---|---|
| `embedded` | on | in-process `Db` / `App`, storage engine |
| `host` | off (implies `embedded`) | `Server`, `Listen`, credential-gated access |
| `client` | off | `Client`, `ClientOptions`, no storage engine |
| `async` | off | Tokio only, no handles by itself |

The last row surprises people, so say it plainly: `async` alone
generates nothing. The pooled `AsyncClient`, `Transaction`, and
`Pooled` types require `client` **and** `async` together. And if your
crate declares a `[features]` table, forward `async` through it.
Generated code contains `cfg(feature = "async")` gates that trip
`unexpected_cfgs` without the forward.

The matrix script covers 6 facade configurations, 8 application and
binary configurations, plus 6 suites with `--tests` (embedded,
client-only, host+client, async, equivalence, facade). Twenty checks,
all green, or the tree isn't.

## Handles

- **`Db` / `App`**: embedded. Reads take `&self`, writes `&mut
  self`. `AppConfig::embedded(path)` opens a file; for a transient
  database call `Db::open_in_memory()` (there is deliberately no
  magic `":memory:"` path; that would just create a file with a
  silly name). `App::from_env()` reads `LIGHTSHADE_MODE`,
  `LIGHTSHADE_PATH`, and `LIGHTSHADE_ENDPOINT`.
- **`Server`**: serves one database (`start`) or many (`start_all`,
  multiplexed by name with per-database credentials; refuses empty
  name lists, duplicate or empty names, in-memory databases, and
  credential-less configs). Configured with `ServeOptions`: idle,
  handshake, and shutdown timeouts (30 s / 10 s / 10 s), 10 s lock
  timeout, 128 connections, 128 transactions, 1 ms group commit,
  1 s credential reload, signals off unless requested.
- **`Client`**: blocking, one stream, one thread at a time (interior
  mutability makes it `Send` but not `Sync`). `ClientOptions`:
  credentials, 5 s connect timeout, 30 s I/O timeout.
- **`AsyncClient` / `Pool`**: `Send + Sync`, all-`&self` methods over
  a pool. `PoolOptions { credentials, connect_timeout: 5 s,
  io_timeout: 30 s, max_connections: 16, max_retries: 3 }`.
  `Pool::new` dials once to prove the endpoint works (and rejects a
  zero-connection pool outright); idle connections are reused,
  transport errors poison a checkout so the next one dials fresh,
  and `Pooled::defunct()` lets error paths poison explicitly.
- **`Transaction`**: the async explicit-transaction handle, with
  *consuming* `commit(self)` / `rollback(self)` and poison-on-drop:
  abandoning one poisons its connection rather than leaking a
  half-applied write. Embedded code uses `&mut` methods plus the
  `Backend::transaction` closure instead
  (same atomicity, no wrapper type).
- **`Follower`**: a read-only tailing handle (`embedded` **and**
  `client` together). `Follower::open(path, endpoint, options)`
  replays the local WAL, `sync_once()` copies the primary's
  un-checkpointed WAL over `WalMeta` / `WalChunk` and reopens
  read-only at the new LSN, `promote()` reopens read-write. A mismatched schema
  fingerprint refuses the sync rather than replaying foreign bytes;
  a killed primary leaves a committed prefix with no holes.

CRUD, transactions (`begin`/`commit`/`rollback`/`transaction`), and
`close` are `Backend` methods, identical across handles. Declared
operations are generated per handle with the same names and types.

## Options reference

| Struct | Key fields (defaults) |
|---|---|
| `OpenOptions` | `read_only: false`, `create: true`, `migrate: false`, `checkpoint_bytes: 8 MiB`, `row_cache_bytes: 64 MiB`, `max_rows/max_wal_bytes: 0` (unlimited); `from_env()` overlays `LIGHTSHADE_CHECKPOINT_BYTES` et al. |
| `RecoverOptions` | `discard_wal: bool`: reopen at the last checkpoint |
| `ServeOptions` | timeouts, limits, group commit, reload interval above; `from_env()` reads `LIGHTSHADE_CREDENTIALS`, `_LISTEN`, `_NETWORK_ACCESS`, `_MAX_ROWS`, `_MAX_WAL_BYTES` and enables signal handling |
| `ClientOptions` | credentials, 5 s / 30 s timeouts; `from_env()` delegates to `ClientCredentials::from_env()` |
| `BackupOptions` | `overwrite: bool` |

## Listeners

```rust
Listen::tcp_local(7171)                    // 127.0.0.1 + loopback-only
Listen::tcp_local_network(addr)            // full SocketAddr + local-network policy
Listen::tcp_external(addr)                 // full SocketAddr + external policy
Listen::parse_tcp("tcp://0.0.0.0:7171", NetworkAccess::External)?
    .with_allowlist(&["10.0.7.0/24"])?     // CIDR, checked pre-handshake
```

`validate()` rejects policy/address mismatches (e.g. `Local` on a
non-loopback bind) before the socket opens, including the wildcard
case, which is explicitly allowed under `LocalNetwork` and refused
under `Local`. Unix sockets are owner-only (`0600`) with stale
predecessors unlinked at bind. `ClientEndpoint::parse` takes
`tcp://HOST:PORT[/DATABASE]`, `unix:///path`, or a bare path;
`tcp` / `tcp_db` constructors and `database()` / `as_path()`
accessors round it out, with `"default"` as the unnamed database.

## Connection URLs

One string holds the endpoint, login, and timeouts, like an ordinary
database URL:

```text
lightshadedb://tester:correct%20horse%20battery%20staple@db.internal:9000/orders
lightshadedb://tester:correct%20horse%20battery%20staple@/run/lightshade.sock
```

`Client::connect_url(&url, options)` (and `AsyncClient::connect_url`)
dials the endpoint inside the URL. Credentials in the URL override the
ones in `ClientOptions`. If the URL has no credentials, the ones in
`ClientOptions` are used. `connect_timeout` / `io_timeout` query parameters
take whole seconds and override the matching option fields:

```text
lightshadedb://tester:s3cret@db.internal:9000/orders?connect_timeout=3&io_timeout=30
```

Characters outside `A-Za-z0-9.-_~` in the id, secret, socket path, or
database name must be `%XX`-encoded (`%20` for the space above).
`LIGHTSHADE_ENDPOINT` also accepts this form in `App::from_env`,
handy as a single config value:

```console
LIGHTSHADE_ENDPOINT=lightshadedb://tester:s3cret@db.internal:9000/orders
LIGHTSHADE_MODE=client
```

Two names are rejected on purpose: `sslmode` (Lightshade DB speaks its own protocol,
not PostgreSQL wire; terminate TLS in front of the server) and
`timezone` (there are no time-zone-aware field types; store instants
as UTC).
