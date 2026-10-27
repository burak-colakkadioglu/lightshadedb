# Query language

Queries are Rust expressions over generated field handles. They lower
to compiler IR and specialize at compile time. There are no query
strings, no runtime planner, and no stringly-typed anything.

## Predicates

```rust
Message::inbox_id.eq(inbox) & Message::is_read.eq(false)
Message::sent_at.between(100..=200)
Message::body.in_(vec!["a".to_string(), "b".to_string()])
Message::priority.is_null()
(Message::a.gt(1) | Message::a.lt(-1)).not()
```

Field comparisons: `eq`, `ne`, `lt`, `le`, `gt`, `ge` (against
values), `eq_field` (against another field, the only join
predicate), `between` (inclusive range), `in_` (value list).
Combinators: `&`, `|`, `!` plus `.and()`, `.or()`, `.not()`.
Nullable columns add `is_null()` / `is_not_null()`. Comparing
against `None` is rejected with a pointer to `is_null()`. The type
system keeps `Option` out of operation parameters entirely.

## Limits are flexible, within reason

`limit` and `offset` accept integer operation parameters *or*
integer literals. Both lower to the same plan. Counts accept only a
filter; ordering or paging a count is a compile error, because a
count has exactly one row and the extra clauses would be lies.

## Three-valued logic, honestly scoped

Comparisons against `NULL` filter the row out instead of erroring:
both sides normalize to `Option<&T>`, `(Some, Some)` pairs compare,
anything else yields false. Same rule across join sources.

But don't mistake `is_null()` for full nullable support. Queries
over nullable columns are still gated at lowering: inserts, gets,
scans, counts, patches, and joins work (nullable join keys use
three-valued equijoin semantics, so `NULL` never matches), but many
other nullable-column queries won't lower yet. If the
compiler refuses your nullable query, it's a missing feature with a
clear error, not a subtle wrong answer. And wrong answers would be
worse.

## Ordering and paging

```rust
order = (Message::sent_at.desc(), Message::id.desc()),
limit = limit,
offset = skip,
```

`asc()` / `desc()` per key. Ordered queries with equality filters
use indexes; range filters use seeks. Ordering on projected queries
must reference output fields, and duplicate order keys are rejected.

## Aggregates and projections

```rust
#[lightshadedb::query(
    from = Message,
    select = Summary::select()
        .total(count())
        .newest(max(Message::sent_at)),
    filter = Message::inbox_id.eq(inbox),
)]
fn summary(&self, inbox: i64) -> Result<Vec<Summary>>;
```

`count()`, `min(field)`, `max(field)`, `sum(field)`, `avg(field)`.
`sum` over text is rejected (`this aggregate requires a numeric
field`). The compiler checks. The runtime never sees it.
`group_by` takes grouped fields (`.group()`); aggregates never return
`NULL`, so empty inputs still produce driver rows.

## Joins

```rust
joins = (Inbox.join_on(Message::inbox_id.eq_field(Inbox::id)),),
```

`TableHandle::join_on` plus `eq_field`. Only `eq_field`;
anything else (`ne_field`, range comparisons) fails at lowering
with the reason spelled out. `select` can then pull fields from
any source.

Conjoin several equalities inside one `join_on` for a multi-key
join: every pair must match. Single-key joins over non-nullable
fields use the usual fast paths (primary-key point lookup or
index seek, shown by `explain`); multi-key joins run as a nested
loop over the key conjunction.

```rust
joins = (Author.join_on(
    Book::author_id.eq_field(Author::id) & Book::lang.eq_field(Author::lang)
),),
```

Nullable keys are allowed and follow equijoin null semantics: a
`NULL` on either side never matches, not even another `NULL`.
Joins over nullable keys always run as a nested loop with that
three-valued comparison, never as a point lookup.

## Types

Fourteen storage types, each with a wire tag (`0..=13`): `i8 i16
i32 i64 u8 u16 u32 u64 f32 f64 bool text bytes uuid`. Typed `NULL`
is tag 14 followed by the declared type's tag, with no payload.
`Field<T, V>` handles carry name and type at compile time, so
`Message::sent_at.eq("nope")` fails to compile instead of failing a
row at runtime. The same types flow through storage, wire, and CSV.
One mapping, everywhere.
