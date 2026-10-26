# Registry and `lightshade.toml`

The registry turns one inventory file into typed multi-database
handles. Instead of opening N databases by hand and keeping the
wiring straight, you declare them and get a struct with one field
per database.

## The inventory file

```toml
[database.a1]
schema = "rust_messages::store"
mode = "embedded"
path = "data/a1.lsdb"

[database.b1]
schema = "rust_messages::store"
mode = "host"
path = "data/b1.lsdb"
listen = "tcp://127.0.0.1:7171"
access = "local"
credentials_file = "data/b1.creds"

[database.c1]
schema = "rust_messages::store"
mode = "client"
endpoint = "tcp://127.0.0.1:7171/b1"

[database.c1.credentials]
id = "tester"
```

Modes: `embedded` (needs `path`), `host` (`path` + `listen` +
`access`, optional allowlist + credentials), `client` (`endpoint` +
credentials). Host entries hold the schema's `Server` (gated on the
`host` feature — missing features are a compile error, not a
surprise); embedded and client entries hold the schema's `App`.

## The macro

```rust
lightshade::registry!("lightshade.toml");
```

You get a `Databases` struct with one typed field per id, `IDS` in
file order, and `open_all()` / `from_env_all()` / `close_all()` to
bring every entry up from literals or environment and shut them
down. `Server::start` serves the `"default"` database;
`Server::start_all` multiplexes named databases with per-database
credentials and fingerprints.

## Environment overrides

Every entry is overridable without touching the file. Note the one
everyone gets wrong: `LIGHTSHADE_<ID>_CREDENTIALS` overrides the
credentials *file path*, not the identity — the id comes from the
TOML `[database.<id>.credentials]` table and the secret from
`LIGHTSHADE_<ID>_SECRET`.

| Variable | Overrides |
|---|---|
| `LIGHTSHADE_<ID>_PATH` | database path |
| `LIGHTSHADE_<ID>_ENDPOINT` | client endpoint |
| `LIGHTSHADE_<ID>_LISTEN` | host bind address |
| `LIGHTSHADE_<ID>_CREDENTIALS` | credentials file path |
| `LIGHTSHADE_<ID>_SECRET` | credential secret |
| `LIGHTSHADE_<ID>_ALLOW` | allowlist networks |

`allow = [...]` sets the TCP CIDR allowlist (host mode only —
anything else is rejected), validated when the macro expands and
enforced pre-handshake after `NetworkAccess`. Entry ids sanitize to
field names (`-` becomes `_`, collisions are an error), and the env
prefix uppercases non-alphanumerics to underscores.

Unknown database names fail identically to bad credentials. That
sentence appears in three chapters of this book because it keeps
mattering: no oracle, no probing, no special cases.
