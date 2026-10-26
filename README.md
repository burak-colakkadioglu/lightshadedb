# Lightshade DB

Lightshade DB is a database you program in Rust instead of querying over
a socket. You declare tables as structs and queries as functions; a proc
macro turns them into specialized storage, indexes, and query loops at
compile time. No SQL strings, no runtime planner, no schema interpreter
on your hot path.

Maintained by [Lightshade](https://github.com/burak-colakkadioglu).

## Why it exists

- Your schema is known at compile time and your queries are fixed, so
  the database should be generated for exactly those, not interpreted
  at runtime.
- You want to start with a local file and maybe grow into hosted,
  multi-client access later, without rewriting application logic.
- Durable by default: crash-safe writes, checksummed storage,
  explicit quotas.

If you need ad-hoc or user-supplied queries, a SQL-speaking database
will serve you better. Lightshade DB only runs queries you declared up
front, in Rust.

## Install

```console
$ cargo install lightshadedb
$ lightshade init notes
$ cd notes
$ cargo run
1 open note(s)
```

## Example

```rust
#[lightshadedb::schema]
pub mod store {
    use lightshadedb::prelude::*;

    pub struct Message {
        #[lightshadedb(primary_key)]
        pub id: i64,
        pub inbox_id: i64,
        pub sent_at: i64,
        pub is_read: bool,
        pub body: String,
    }

    #[lightshadedb::operations]
    pub trait Messages {
        #[lightshadedb::query(
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
            order = (Message::sent_at.desc(), Message::id.desc()),
            limit = limit,
        )]
        fn recent_unread(&self, inbox: i64, limit: i64) -> Result<Vec<Message>>;
    }
}
```

```rust
let mut db = store::Db::open("notes.lsdb")?;
db.insert(store::Message { id: 1, inbox_id: 2, sent_at: 3, is_read: false, body: "hi".into() })?;
let unread = db.recent_unread(2, 10)?;
```

## Deployment modes

| Feature  | What you get                                              |
|----------|-----------------------------------------------------------|
| `embedded` | In-process database, durable on disk (WAL + fsync)      |
| `host`     | Serve it over TCP or Unix sockets                       |
| `client`   | Connect from a process with no storage engine at all    |
| `async`    | Pooled async client                                     |

Code written against the `Backend` trait compiles and runs in all of
them. Deployment is a `Cargo.toml` edit, not a rewrite.

## Learn more

The book is the documentation. Build it and start here:

```console
$ mdbook build docs
```

- [Your first database](docs/src/tutorials/first-database.md): the
  ten-minute tutorial. Start here.
- [Deployment modes](docs/src/reference/deployment-modes.md): which
  feature to pick and why.
- [How-tos](docs/src/howto/backups-and-restore.md): backups, CSV and
  dumps, quotas, incident drill.
- [Reference](docs/src/reference/cli.md): macro, queries, CLI, wire
  protocol, bytes on disk, errors.
- [Explanation](docs/src/explanation/architecture.md): why it looks
  this way.

Runnable code lives in [`examples/`](examples/).

## Developing

The tree stays green: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, workspace tests, and
`scripts/matrix.sh --tests` pass on every commit. No comments in code,
ever. Explain it in the book instead.

See [CONTRIBUTING](CONTRIBUTING) (including the relicensing grant) and
the [Code of Conduct](CODE_OF_CONDUCT.md). Use the issue
templates for bug reports and feature requests, and the pull request
template for changes.

## License

Dual-licensed MIT OR Apache-2.0. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE-2.0](LICENSE-APACHE-2.0).
