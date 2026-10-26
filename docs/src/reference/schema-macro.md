# Schema macro

One `#[lightshadedb::schema]` module declares tables, projections, and
*up to* one `#[lightshadedb::operations]` trait. A second operations
trait is a compile error; zero is fine (you get storage and CRUD with
no declared queries). The macro generates field handles, patch types,
row codecs, indexes, and the `Backend`, `App`, `Client`, `Server`,
and pool handles.

```rust
#[lightshadedb::schema]
pub mod store {
    use lightshadedb::prelude::*;

    pub struct Inbox { /* ... */ }

    #[lightshadedb(skip)]
    pub struct UnreadDigest { /* projection: never stored */ }

    #[lightshadedb::operations]
    pub trait Messages { /* ... */ }
}
```

Tables are structs with named fields. Skipped structs are
projections for `select` queries — never stored, no primary key
needed.

## The rules, up front

The compiler enforces these and names the offender every time:

- Exactly one `#[lightshadedb(primary_key)]` per stored table. Zero or
  two fails; the diagnostic names the table.
- No nullable primary key. No nested `Option<Option<T>>`.
- New non-nullable fields need defaults at migration time.
- No duplicate tables, fields, or operations; no unknown fields in
  queries; primary keys can't carry defaults.

## Field types

| Rust type | Storage | Notes |
|---|---|---|
| `i8`…`i64`, `u8`…`u64` | fixed-width integers | range-checked at the boundary |
| `f32`, `f64` | IEEE floats | ordered totally |
| `bool` | one byte | |
| `String` | UTF-8 text | short values inline, large ones spilled |
| `Vec<u8>` | bytes | |
| `[u8; 16]` | UUID | arrays of exactly 16 bytes map to `Uuid`; there is no named `Uuid` type |
| `Option<T>` | nullable | three-valued logic in queries |

No `NULL` literals and no `Option` operation parameters.
Nullability comes from columns and is tested with `is_null()` /
`is_not_null()`.

## Operations

```rust
#[lightshadedb::operations]
pub trait Messages {
    #[lightshadedb::query(filter = ..., order = ..., limit = ..., offset = ...)]
    fn page(&self, ...) -> Result<Vec<Row>>;

    #[lightshadedb::query(count, filter = ...)]
    fn how_many(&self, ...) -> Result<i64>;

    #[lightshadedb::query(from = T, select = ..., joins = ..., group_by = ...)]
    fn report(&self, ...) -> Result<Vec<Projection>>;

    #[lightshadedb::update(filter = ..., set = (T::f.set(v),))]
    fn fix(&mut self, ...) -> Result<u64>;

    #[lightshadedb::delete(filter = ...)]
    fn purge(&mut self, ...) -> Result<u64>;
}
```

Reads take `&self` and return rows (`Vec<Row>`, or `i64` for `count`).
Writes take `&mut self` and return affected-row counts. `update`
*requires* `set` — unbounded rewrites are compile errors, not
footguns. `delete` takes an optional filter. `count` accepts *only* a
filter: no ordering, paging, joins, grouping, or projections on a
count.

Joins only accept `eq_field` — `ne_field` exists on field handles but
the lowering rejects anything but equi-joins, because only equality
preserves the index story. Joins over nullable keys are rejected for
the same reason: nullability would make the predicate three-valued
and the compiler refuses to guess.

## What gets generated

For a module `store`:

- Row structs with `to_values` / `from_values` codecs and `RowPatch`
  types (one `Option` field per column, `Default`).
- Typed field handles: `Message::id`, `Message::id.eq(...)`, ordering,
  grouping, and select builders.
- `store::Backend`: the `lightshadedb::Backend` implementation shared by
  every handle.
- `store::App` + `AppConfig`: `AppConfig::embedded(path)` takes only
  a path (options come from `OpenOptions` at open time);
  `App::open(config)` and `App::from_env()` (`LIGHTSHADE_MODE`,
  defaulting to `embedded` when that feature is on, else `client`,
  then `LIGHTSHADE_PATH` or `LIGHTSHADE_ENDPOINT` plus
  credentials).
- `store::Client` (+ `ClientOptions`) under `client`,
  `store::Server` (+ `ServeOptions`) under `host`, and the async
  `AsyncClient` / `Transaction` / `Pooled` types under
  `client` **and** `async` together — `async` alone generates no
  handles.

## Diagnostics

Schema violations name the table, the field, and the rule
(`duplicate field`, `missing primary key`, `nullable primary key`,
`unknown field`, `duplicate operation`). The compile-fail suite pins
the important messages — enough that refactoring the macro can't
silently degrade them, though not literally every diagnostic has a
case. If you hit an unpinned message that's confusing, that's a bug
report, not a rite of passage.
