# CLI reference

The `lightshade` binary does two jobs: the legacy `.lightshade`
workload path (parse, generate, build, and run interface binaries)
and direct administration of databases. Usage differs per command,
so read the arity, not just the name.

```console
$ lightshade <command> <workload> <database> [args...]
```

That shape is a lie for four commands: `check`, `generate`, `build`,
and `explain` take no database; `init` takes a destination path;
`--version` takes nothing.

## Commands

| Command | Effect |
|---|---|
| `check <workload>` | Parse and validate; prints `ok: N tables, M operations` plus per-item lines |
| `generate <workload> [--out <file.rs>]` | Emit specialized Rust (default out: `target/lightshade/generated.rs`) |
| `build <workload> [--out <file>]` | Compile and cache the interface binary; without `--out`, prints the cached path |
| `run <workload> <db\|sock> <op> [name=value ...]` | Execute one operation; `.sock` paths connect, `.lsdb` paths open |
| `serve <workload> <db>` | Host over a Unix socket (TCP only via `LIGHTSHADE_LISTEN`) |
| `migrate <workload> <db> [--dry-run \| --apply [--allow-destructive] [--no-backup]]` | Plan or apply a migration |
| `verify <workload> <db>` | Structural check; modifies nothing |
| `inspect <workload> <db>` | Schema, sizes, LSN, row counts |
| `compact <workload> <db>` | Rewrite snapshot, truncate WAL |
| `recover <workload> <db> [--discard-wal]` | Repair a torn tail, or quarantine-and-drop the WAL |
| `backup <workload> <db> <dest>` | Consistent copy (database *and* WAL sidecar); refuses existing destinations |
| `dump <workload> <db> <dest.dump>` | Portable self-describing backup; refuses to overwrite |
| `restore <workload> <db> <src.dump>` | Rebuild from a dump; refuses existing databases |
| `export <workload> <db> <table> <dest.csv>` | One table to CSV (empty field means NULL); destination must not exist |
| `import <workload> <db> <table> <src.csv>` | Append CSV rows into one table |
| `explain <workload> [operation]` | Access paths, computed at compile time, no database needed |
| `init <path> [--name …] [--features …] [--path …] [--force]` | Scaffold an app (features: `embedded`, `host`, `client`; `host` implies `embedded`; `--path` has a `--lightshade-path` alias) |
| `--version` | Print the version |

## Exit codes

Stable and asserted: `0` success, `2` usage error (anything starting
with `usage:`), `1` runtime failure. Library errors add 3 through 8
(corruption, conflict, auth, quota, unsupported, migration; see
[Error model](error-model.md)). Scripts should branch on the code,
not on stderr text.

`--help` is canonical. If this page and `--help` ever disagree,
`--help` wins and the page is a bug. Report both texts.

## Notes worth knowing

- **`run`** prints a `# col…` header only for multi-row results, and
  the workload argument is always a `.lightshade` file (e.g.
  `examples/messages.lightshade`), never a bare name.
- **`migrate`** without flags delegates to the interface's migrate;
  `--apply` quarantines a timestamped backup first (WAL included)
  and refuses to run destructive plans without `--allow-destructive`.
- **`recover`** replays by default; `--discard-wal` quarantines the
  WAL aside and only deletes it on success, restoring it on failure.
- **`explain`** with no operation explains everything. Handy in CI
  to catch plan regressions.
- **`build`** caching means repeat `run` invocations skip `rustc`;
  the cache lives `0700` and refuses symlinks.
- **`init`** writes the manifest itself with a versioned dependency,
  so don't hand-write `Cargo.toml` first and scaffold second.
  Re-init without `--force` errors.
