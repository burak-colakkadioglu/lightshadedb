# Your first database

You'll go from an empty directory to a working embedded database with
a declared query. You need a Rust toolchain and about ten minutes.

> **Crate name.** On release, Lightshade DB ships on crates.io as
> `lightshadedb`. The code below uses the release dependency; Rust
> import paths (`lightshadedb::…`, `#[lightshadedb::…]`) still show the
> pre-release namespace and track the rename when it lands.

## 1. Scaffold

Install the CLI and generate a starter project:

```console
$ cargo install lightshadedb
$ lightshade init notes
$ cd notes
$ cargo run
1 open note(s)
```

`init` writes you a crate: a manifest with a versioned dependency
(no path hacks, no git clones), a `store` module with one table and
one query, and a `main.rs` that inserts a row and reads it back. It
takes `--name`, `--features` (`embedded`, `host`, or `client` —
`host` pulls in `embedded` automatically), and `--force` to overwrite
an existing directory. If you'd rather wire the manifest yourself:

```toml
[dependencies]
lightshadedb = "2"

[features]
default = ["embedded"]
embedded = ["lightshadedb/embedded"]
host = ["embedded", "lightshadedb/host"]
client = ["lightshadedb/client"]
async = ["lightshadedb/async"]
```

One rule that bites everyone once: if your crate declares a
`[features]` table, forward `async`. Generated code contains
`cfg(feature = "async")` gates, and without the forward you'll get
`unexpected_cfgs` noise instead of a clean build.

Running the scaffold leaves a `notes.lsdb` file in the directory you
ran from (the path is relative to your working directory, not the
binary). That file plus its sidecars *is* the database.

## 2. Declare a table

Tables are plain structs. One field carries the primary key, and the
compiler is picky in exactly the ways that save you later:

```rust
pub struct Message {
    #[lightshadedb(primary_key)]
    pub id: i64,
    pub inbox_id: i64,
    pub sent_at: i64,
    pub is_read: bool,
    pub body: String,
    pub priority: Option<i64>,
}
```

Exactly one primary key — zero or two is a compile error, and it names
the table. `Option<T>` fields are nullable end to end, but there are
two rejections to know about now: no nullable primary keys, and no
nested `Option<Option<T>>`. Both fail at compile time with the field
named.

## 3. Declare operations

One trait per schema module, holding every operation. Queries read
through `&self` and return rows; writes take `&mut self` and return
affected-row counts:

```rust
#[lightshadedb::operations]
pub trait Messages {
    #[lightshadedb::query(
        filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
        order = (Message::sent_at.desc(), Message::id.desc()),
        limit = limit,
    )]
    fn recent_unread(&self, inbox: i64, limit: i64) -> Result<Vec<Message>>;

    #[lightshadedb::query(count, filter = Message::inbox_id.eq(inbox))]
    fn inbox_count(&self, inbox: i64) -> Result<i64>;

    #[lightshadedb::update(
        filter = Message::id.eq(message),
        set = (Message::is_read.set(true),),
    )]
    fn mark_read(&mut self, message: i64) -> Result<u64>;

    #[lightshadedb::delete(filter = Message::sent_at.lt(cutoff))]
    fn purge_before(&mut self, cutoff: i64) -> Result<u64>;
}
```

Two guardrails worth internalizing: `update` *requires* a `set`
clause, so you can't accidentally rewrite a table, and `count`
queries reject everything except a filter — no ordering, no paging, no
joins on a count. The full rulebook is [Schema macro](../reference/schema-macro.md).

## 4. Open and use

```rust
use lightshadedb::prelude::*;

let mut app = store::App::open(store::AppConfig::embedded("notes.lsdb"))?;
app.insert(store::Message { id: 1, inbox_id: 7, sent_at: 100, is_read: false, body: "hello".into(), priority: None })?;
assert_eq!(app.inbox_count(7)?, 1);
assert_eq!(app.mark_read(1)?, 1);
assert_eq!(app.recent_unread(7, 10)?.len(), 0);
```

Want a scratch database that vanishes? There's no magic `":memory:"`
path — that would just create a file with a silly name. Generated
handles have a dedicated constructor for it:

```rust
let mut app = store::Db::open_in_memory()?;
```

For configuration instead of literals, `App::from_env()` reads
`LIGHTSHADE_MODE` (defaulting to `embedded` when that feature is on,
else `client`), then `LIGHTSHADE_PATH` for embedded or
`LIGHTSHADE_ENDPOINT` plus credentials for client. Same binary,
different environment, no recompilation.

## 5. Transactions

The closure form commits on `Ok` and rolls back on `Err` — use it by
default:

```rust
app.transaction(|db| {
    db.insert(store::Message { id: 2, inbox_id: 7, sent_at: 200, is_read: false, body: "again".into(), priority: None })?;
    db.mark_read(1)?;
    Ok(())
})?;
```

`begin` / `commit` / `rollback` exist for the cases where a closure
doesn't fit. Either way, a dropped or failed transaction leaves no
half-applied writes behind.

## Where next

- [Hosting and clients](hosting-and-clients.md) puts this database on
  a socket and connects to it.
- [Schema macro](../reference/schema-macro.md) documents every
  attribute and every rule the compiler enforces.
- [Query language](../reference/query-language.md) covers filters,
  joins, aggregates, and nullable semantics.
