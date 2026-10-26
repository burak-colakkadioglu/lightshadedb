# Hosting and clients

Your schema already compiles to a server and to clients — no new
code, just new handles. This page serves the database from the last
tutorial and connects to it three ways.

## Serve it

The generated `Server` binds a `Listen` endpoint. Loopback-only is
the default posture; anything wider is explicit:

```rust
let listen = lightshadedb::Listen::tcp_local(7171);
```

`tcp_local` binds loopback with a loopback-only policy.
`tcp_local_network` and `tcp_external` take a full `SocketAddr` and
pair it with the matching policy — including wildcard binds, which
are allowed under `LocalNetwork` and refused under `Local`. For
anything internet-facing, parse and constrain:

```rust
let listen = lightshadedb::Listen::parse_tcp("tcp://0.0.0.0:7171", lightshadedb::NetworkAccess::External)?
    .with_allowlist(&["10.0.7.0/24"])?;
```

`parse_tcp` requires the `tcp://` prefix and resolves hostnames; the
allowlist is checked per peer *before* the handshake, after the
access policy. `validate()` rejects policy/address mismatches before
a socket ever opens, so misconfiguration fails at startup, not at
2 AM.

The legacy CLI also serves, with narrower syntax — it takes a
workload file and a database, binds a Unix socket by default, and
only speaks TCP when `LIGHTSHADE_LISTEN` says so:

```console
$ lightshade serve messages.lightshade messages.lsdb
```

## Connect to it

Endpoints come in three shapes: `tcp://HOST:PORT[/DATABASE]`,
`unix:///path/to.sock`, or a bare filesystem path (a socket or an
endpoint file). The bracket form covers IPv6 (`tcp://[::1]:9000`).
The database segment selects which database on that listener you
authenticate against; path endpoints always mean `"default"`.

A blocking `Client` mirrors the embedded API over one stream. It's
`Send` but not `Sync` — think one client per thread, built on an
interior `RefCell`, configured with `ClientOptions` (credentials,
5-second connect timeout, 30-second I/O timeout). For sharing across
async tasks, reach for the pool:

```rust
let pool = lightshadedb::Pool::new(
    lightshadedb::ClientEndpoint::parse("tcp://db.internal:7171/orders")?,
    workload_fingerprint,
    lightshadedb::PoolOptions::new(creds),
).await?;
```

`Pool::new` dials once to prove the endpoint works, then manages up
to 16 connections (configurable) with idle reuse. Transport errors
poison a connection so the next checkout dials fresh instead of
handing you a corpse. Retries apply to pooled single operations only:
errors that say they're retryable get retried up to `max_retries`,
honoring `retry_after`; timeouts are never retried, and transactions
surface errors instead of retrying.

Pool handles need *both* the `client` and `async` features. `async`
alone buys you a Tokio dependency and nothing else — the generated
`AsyncClient`, `Transaction`, and `Pooled` types sit behind
`all(feature = "client", feature = "async")`.

## Credentials

Servers hold a credential table; clients present an id and a secret.
Roles are reader, writer, admin. Secrets compare in constant time,
unknown databases fail exactly like bad credentials (so nobody can
probe for database names), and `ClientCredentials::from_env` reads
`LIGHTSHADE_CREDENTIAL_ID` and `LIGHTSHADE_CREDENTIAL_SECRET`. The
threat model lives in [Security](../explanation/security.md).

## Which handle when

| Handle | Sharing | Storage | Reach for it when |
|---|---|---|---|
| `Db` / `App` | single owner | local file | the process owns the data |
| `Server` | multi-threaded | local file | the process owns the data and shares it |
| `Client` | one per thread | none | a thread-per-connection shape fits |
| `AsyncClient` / `Pool` | shared, `Send + Sync` | none | async tasks share access |

All four implement `Backend`, so generic code (`fn run(app: &mut
impl Backend)`) runs unchanged everywhere. The one asymmetry to
respect: over the wire, big scans and bulk imports deserve the pool,
not a lone stream.
