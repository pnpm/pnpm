---
"@pnpm/pnpr": patch
---

The libsql auth backend's `replicaPath` setting now works with Turso Cloud, as it does with a self-hosted `sqld`. pnpr used to fail to start against Turso Cloud with an embedded replica configured [pnpm/tasks#183](https://github.com/pnpm/tasks/issues/183).
