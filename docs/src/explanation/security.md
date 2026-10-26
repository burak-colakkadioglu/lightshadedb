# Security

Lightshade DB listens on sockets, so it treats the network as hostile
by default: loopback-only unless configured otherwise,
authentication before any operation, and no oracle that distinguishes
"wrong password" from "no such database."

## Exposure

TCP listeners carry an explicit `NetworkAccess` policy: `Local`
(loopback), `LocalNetwork` (loopback, RFC1918, link-local, IPv6
unique-local), or `External` (any peer, and you have to ask for it by
name). The policy is validated against the bind address before
listening and enforced per peer before the handshake. An optional
CIDR allowlist narrows further; Unix-socket peers have no IP and are
local by construction.

Unix sockets and endpoint files are created owner-only (`0600`),
stale sockets unlinked at bind. A `0600` check on backup files is
part of the restore drill, not decoration. Permissions are a
runtime property here, verified the same way data is.

## Authentication

Credential tables map ids to roles: reader, writer, admin. Secrets
are zeroized in memory, compared in constant time, and loaded from
files or environment. Never baked into binaries. The handshake binds
workload fingerprint, database name, and credential in one exchange,
and a mismatch in any of them yields the same `AuthError::Failed`.
Unknown databases fail identically to bad credentials, so nobody
can probe for database names through the handshake.

TLS termination, credential rotation, and session revocation are host
configuration: reloads apply without dropping established sessions
until they re-authenticate, and revoked sessions fail closed. If
you're terminating TLS, terminate it in front and let the database
worry about the database.

## Audit and least privilege

- `LIGHTSHADE_AUDIT_LOG` records connection lifecycle as JSONL. It's
  observability, not a recovery log. Never parse it for recovery,
  and rotate it like any other log.
- Run servers as a dedicated user with write access to exactly one
  directory: database, WAL, lock, socket, and spill all live there.
  Nothing else needs writing, so nothing else should be writable.
- Client processes hold no storage engine and no data files: only
  endpoint addresses and secrets. That's the whole point of the
  `client`-only build: a compromised client yields queries, not the
  database.
- Slow-query logging (`LIGHTSHADE_SLOW_LOG`, `LIGHTSHADE_SLOW_MS`)
  records operation, strategy, timing, and rows. Never parameters.
  Your data doesn't belong in logs.
