---
"@pnpm/pnpr": patch
---

The libsql auth backend's `replicaPath` setting now works with Turso Cloud. pnpr failed to start against Turso Cloud with an embedded replica configured, because Turso no longer serves the sync protocol the replica used. The replica now uses the protocol the primary offers, so the same setting works with Turso Cloud and with a self-hosted `sqld` [pnpm/tasks#183](https://github.com/pnpm/tasks/issues/183).
