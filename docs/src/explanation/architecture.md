# Architecture

## Pipeline

```
Rust schema ──▶ macro ──▶ lowered IR ──▶ validate ──▶ plan ──▶ emit ──▶ specialized Rust
                                          │                      │
                                    parse_workload           templates
```

**Parse** turns `.lightshade` text or macro input into workload IR:
tables, fields, types, operations. **Validate** enforces every
structural rule exactly once — primary keys, nullable restrictions,
operation shapes, name resolution. **Plan** picks indexes, counters,
and key expressions per operation and fingerprints the schema and
workload. **Emit** splices runtime templates (storage engine,
admin, slow-query guard) with generated per-table code: row structs,
codecs, query loops, CRUD, transactions, wire handlers.

Generated code depends only on `std` and the facade crate, compiled
with plain `rustc` and no external crates. That's why the emitted
code is `std`-only and why file modes live behind a named constant:
macro spans trip the non-octal-permissions lint otherwise, and the
generated code has to be lint-clean from birth.

## Planner

The planner is static analysis, not a cost model. For each operation
it collects equality constraints from the filter, matches them
against declared key orderings, and picks the tightest access path:
point lookup, range seek, or scan with a maintained counter where
one exists. Advanced queries (joins, grouping, multi-source
predicates) resolve every operand to its source before emitting
nullable-aware comparisons. There is nothing to tune and nothing to
`ANALYZE`. If the plan is wrong, the schema or the query is wrong —
and `explain` shows the chosen path per operation, so you can see
that for yourself.

## One lowering, every handle

`Db`, `Client`, `AsyncClient`, and `Transaction` share the operation
IR; only transport differs (direct call, single stream, pooled
stream). The blocking and async transports share the wire codec
(`encode_hello`, `decode_handshake_response`, `encode_request`,
`decode_response`), so both emit byte-identical handshakes and
frames. A byte the blocking client sends is a byte the async client
would send — the compatibility suite pins this rather than trusting
it.

## Where the code lives

- `macros/` — proc macros: schema model, lowering, per-handle
  expansion (`expand/{db,table,ops,backend,client,server,app,
  migration,types}`).
- `codegen/` — the compiler: IR (`ir/`), planner (`plan.rs`),
  emitters (`common`, `persist/`, `emit_write`, `emit_query`),
  legacy interface (`compiler/interface/`), templates
  (`compiler/runtime/`).
- `src/` — the facade: public types, `Backend`, pool, dataplane,
  wire codec, CLI binary.

The dependency rule is one-directional: `src` and `macros` consume
`codegen`, never the reverse. If you find yourself wanting the
compiler to know about a handle, that's the design telling you the
handle code goes in the macro instead.
