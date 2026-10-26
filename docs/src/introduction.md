# Introduction

Lightshade DB is a database you program in Rust instead of querying over
a socket. Tables are structs, queries are expressions over generated
field handles, and a proc macro turns all of it into specialized
storage, indexes, and query loops at compile time. No SQL strings, no
runtime planner, no schema interpreter sitting on your hot path.

```rust
#[lightshade::schema]
pub mod store {
    use lightshade::prelude::*;

    pub struct Message {
        #[lightshade(primary_key)]
        pub id: i64,
        pub inbox_id: i64,
        pub sent_at: i64,
        pub is_read: bool,
        pub body: String,
    }

    #[lightshade::operations]
    pub trait Messages {
        #[lightshade::query(
            filter = Message::inbox_id.eq(inbox) & Message::is_read.eq(false),
            order = (Message::sent_at.desc(), Message::id.desc()),
            limit = limit,
        )]
        fn recent_unread(&self, inbox: i64, limit: i64) -> Result<Vec<Message>>;
    }
}
```

That's the whole trick. The macro reads that module and generates a
storage engine shaped exactly like your schema, plus typed handles for
every deployment you might want later.

## When it fits

You have a Rust service with durable, queryable local state: inboxes,
catalogs, ledgers, queues, session stores. Your schema is known at
compile time and your queries are fixed — you can list them, because
you're about to write each one down as a function. You want to start
with a file and maybe grow into hosted multi-client access without
rewriting application logic.

## When it doesn't

You need ad-hoc or user-supplied queries. There is no query string
and no runtime planner; every operation is declared in Rust up front.
You need non-Rust callers — the only stable interface is the Rust API
(plus the `.lightshade` CLI path, which is a development tool, not a
protocol promise). You need schema-less data — every field has one of
fourteen storage types, and the compiler will tell you about it.

## One schema, every deployment

`embedded` gives you an in-process database. `host` serves it over
sockets. `client` connects from a process with no storage engine at
all. `async` adds a pooled async client. Application code written
against the `Backend` trait compiles and runs in all of them, which
means the deployment decision stays a `Cargo.toml` edit instead of a
rewrite. [Deployment modes](reference/deployment-modes.md) has the
matrix.

## How to read this book

Tutorials teach by doing: build one database, host it, evolve its
schema. How-tos solve specific problems: backups, interchange,
quotas, and what to do at 3 AM when something breaks. Reference is
exhaustive on purpose — the macro, the queries, the CLI, the wire,
the bytes on disk, the errors. Explanation covers why it looks this
way: the pipeline, the guarantees, the threat model.

If you're new, start with [Your first database](tutorials/first-database.md)
and don't skip it. Everything else assumes you've opened one.
