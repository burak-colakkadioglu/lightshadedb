# Backups and restore drills

There is no incremental backup and no online snapshot. There is a
whole-file copy, a verifier, and a drill you run until it's boring.
Boring is the goal.

## Take a backup

```console
$ lightshade backup messages.lightshade messages.lsdb backup.lsdb
$ lightshade verify messages.lightshade backup.lsdb
```

`backup` copies the database *and* its WAL sidecar, fsyncs the
result, and refuses to overwrite an existing destination. Pick a
fresh name or move the old one aside. `verify` replays snapshot and
WAL structurally and modifies nothing. Backup files land owner-only
(`0600`); confirm with `stat -c '%a'` and keep it that way.

## The drill

This is the project's own `restore-drill.sh`, compressed. Run it
against production-shaped data on a schedule:

1. Insert a canary row, `backup`, `verify` the backup.
2. Checksum it (`sha256sum`), then simulate the fire: delete the
   database, its WAL, and its lock file.
3. `install -m 0600 backup.lsdb messages.lsdb`: plain copy, mode set
   explicitly. `verify`, then read the canary back.
4. Now the parts people skip: recover a torn WAL and confirm the
   committed prefix survives, then damage a snapshot copy and confirm
   `verify` *fails* on it. The negative control matters. A verifier
   you've never seen fail is a rumor.

A backup that has never been restored is also a rumor. The drill is
the backup.

## Torn writes and damaged WALs

A crash mid-write can tear the WAL tail. Recovery truncates to the
last committed prefix and replays from there: acknowledged writes
(fsync'd before the crash) survive, unacknowledged tail bytes don't.
If the WAL is damaged beyond the tail, `recover --discard-wal`
quarantines the WAL aside and reopens at the last checkpoint.
Everything after the checkpoint is gone, which is exactly the loss
window your drill taught you to expect. A damaged *snapshot* is never
repaired in place. Restore from backup.

## What to watch

- **WAL size vs `LIGHTSHADE_CHECKPOINT_BYTES`** (default 8 MiB): a
  WAL that outgrows your backup window means checkpoints aren't
  keeping up (see [Quotas and limits](quotas-and-limits.md)).
- **Lock timeouts** (`LIGHTSHADE_LOCK_TIMEOUT_MS`): usually a second
  writer, sometimes a stale lock after a crash. If no process is
  alive, remove the `.lock` file and reopen.
- **Spill dir space**: spill is bounded, but a full filesystem fails
  writes loudly rather than corrupting data. Loud beats silent.
