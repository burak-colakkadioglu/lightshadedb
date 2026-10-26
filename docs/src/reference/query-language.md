# Query language

Queries are Rust expressions over generated field handles. They lower
to compiler IR and specialize at compile time — there are no query
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
values), `eq_field` (against another field — the only join
predicate), `between` (inclusive range), `in_` (value list).
Combinators: `&`, `|`, `!` plus `.and()`, `.or()`, `.not()`.
Nullable columns add `is_null()` / `is_not_null()`. Comparing
against `None` is rejected with a pointer to `is_null()` — the type
system keeps `Option` out of operation parameters entirely.

## Limits are flexible, within reason

`limit` and `offset` accept integer operation parameters *or*
integer literals — both lower to the same plan. Counts accept only a
filter; ordering or paging a count is a compile error, because a
count has exactly one row and the extra clauses would be lies.

## Three-valued logic, honestly scoped

Comparisons against `NULL` filter the row out instead of erroring:
both sides normalize to `Option<&T>`, `(Some, Some)` pairs compare,
anything else yields false. Same rule across join sources.

But don't mistake `is_null()` for full nullable support. Queries
over nullable columns are heavily gated at lowering: inserts, gets,
scans, counts, and patches work, but nullable join keys are rejected
outright and many nullable-column queries won't lower yet. If the
compiler refuses your nullable query, it's a missing feature with a
clear error, not a subtle wrong answer — and wrong answers would be
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
field`) — the compiler checks, the runtime never sees it.
`group_by` takes grouped fields (`.group()`); aggregates never return
`NULL`, so empty inputs still produce driver rows.

## Joins

```rust
joins = (Inbox.join_on(Message::inbox_id.eq_field(Inbox::id)),),
```

`TableHandle::join_on` plus `eq_field` — and only `eq_field`.
`select` can then pull fields from any source. Anything else
(`ne_field`, nullable keys) fails at lowering with the reason
spelled out.

## Types

Fourteen storage types, each with a wire tag (`0..=13`): `i8 i16
i32 i64 u8 u16 u32 u64 f32 f64 bool text bytes uuid`. Typed `NULL`
is tag 14 followed by the declared type's tag, with no payload.
`Field<T, V>` handles carry name and type at compile time, so
`Message::sent_at.eq("nope")` fails to compile instead of failing a
row at runtime. The same types flow through storage, wire, and CSV —
one mapping, everywhere.
