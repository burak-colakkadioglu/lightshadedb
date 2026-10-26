# Evolving a schema

Schemas change; the discipline is always the same: change the code,
preview the plan, apply it. Never skip the middle step.

## 1. Change the schema

Add a field, rename something, add a table:

```rust
pub struct Message {
    #[lightshadedb(primary_key)]
    pub id: i64,
    pub inbox_id: i64,
    pub sent_at: i64,
    pub is_read: bool,
    pub body: String,
    pub priority: i64, // new: needs a default or must be nullable
}
```

New non-nullable fields need defaults or the migration is rejected —
there's nowhere for existing rows' values to come from otherwise.
Dropping a column or a table is destructive, full stop, and the
tooling treats it that way.

## 2. Preview the plan

```console
$ lightshade migrate messages.lightshade messages.lsdb --dry-run
```

The dry run prints an ordered `MigrationPlan` — add, rename, and drop
steps for tables and fields — and changes nothing. Read it like a
diff: `has_destructive_steps()` tells you whether anything is lost,
`is_empty()` tells you whether anything happens at all.

## 3. Apply it

```console
$ lightshade migrate messages.lightshade messages.lsdb --apply
```

Unless `--no-backup` is passed, the database is backed up first (to a
timestamped `pre-migrate` sibling). Destructive plans additionally
require `--allow-destructive`; without it the command refuses, no
matter how sure you are. Programmatic migration follows the same
shape — `MigrationOptions::from_env()` (honoring
`LIGHTSHADE_MIGRATION_BACKUP` and
`LIGHTSHADE_ALLOW_DESTRUCTIVE_MIGRATION`), then `check` the plan,
then apply. Renames across differently-named stored tables go through
rename hints.

## What actually gets checked

Two independent gates stand between your build and old data. The
*version* gate accepts the current snapshot and WAL generations plus
their legacy predecessors and rejects everything else as an
`UnsupportedVersion` corruption. The *fingerprint* gate compares the
compiled schema hash against the stored one, field order included —
order matters, so reordered fields are a rename, not a no-op. Either
gate failing means migration, not repair: `SchemaMismatch` is the
database telling you your code changed, and it's right.

Old snapshots stay readable across generations, and the WAL replays
both current and legacy record layouts, so the common cases — added
tables, added defaulted fields — migrate with no drama and no hints.
