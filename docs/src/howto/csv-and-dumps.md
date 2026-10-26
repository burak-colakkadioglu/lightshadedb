# Dumps and CSV interchange

Two formats, both directions, identical bytes whether you use the CLI
or the Rust API — the compatibility suite pins that byte-for-byte, so
pick whichever side is convenient and stop worrying.

## Portable dumps (`LIGHTDMP` v1)

A self-describing binary: full column descriptors followed by rows.
Dumps restore into any compatible build on any platform.

```console
$ lightshade dump messages.lightshade messages.lsdb messages.dump
$ lightshade restore messages.lightshade fresh.lsdb messages.dump
```

Both commands refuse to overwrite: dump needs a fresh destination,
and restore refuses a database that already exists. In Rust, the same
operations hang off your generated handle:

```rust
db.dump("messages.dump")?;
store::Db::restore("fresh.lsdb", "messages.dump")?;
```

Underneath, `write_dump_file` / `read_dump_file` move `DumpTable`
descriptors (`name`, `columns`, `rows`) and `check_tables` validates
a dump's descriptors against an expected schema *before* you commit
to restoring it. Use it when accepting dumps from anywhere
untrusted.

Restores are strict and non-transactional: a malformed dump fails the
whole restore instead of applying half of it. That's a feature — a
half-restored database is worse than no restore.

## CSV

One table per file, header row plus one record per row, RFC-4180
quoting (`""` escapes a quote; quoted fields may span lines; empty
field means `NULL`):

```console
$ lightshade export messages.lightshade messages.lsdb messages messages.csv
$ lightshade import messages.lightshade fresh.lsdb messages messages.csv
```

Note the table name is `messages`, plural, exactly as declared —
`message` fails with `unknown table`, and rightly so. In Rust:

```rust
db.export_csv::<store::Message>("messages.csv")?;
db.import_csv::<store::Message>("messages.csv")?;
```

Two sharp edges, both deliberate. Export uses `create_new`
semantics: the destination must not exist, so rotate old exports out
of the way first. Import reads the whole file into memory before
validating types — the CLI streams rows, the `Backend` API path does
not — so size your imports (and your machines) accordingly.

## Choosing between them

| | Dump | CSV |
|---|---|---|
| Schema carried? | yes, full descriptors | no, positional by header |
| Whole database? | yes, all tables | no, one table per file |
| Human-readable? | no | yes |
| Failure mode | whole restore fails | type errors fail the import |

## Reports

Every operation returns a report — `DumpReport`, `RestoreReport`,
`ExportReport`, `ImportReport` — with row counts and destinations.
Assert on them in pipelines. "Export said 10,000 rows, import says
9,999" is a discrepancy you want to catch in code, not discover in
an audit.
