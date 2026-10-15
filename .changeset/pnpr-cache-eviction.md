---
"@pnpm/pnpr": minor
---

pnpr now supports cache size caps, TTL retention, and orphan namespace pruning. The `cache:` setting accepts `maxSize` and `maxAge` limits to bound upstream proxy storage and evict stale files. Startup sweeps prune orphaned upstream namespaces left behind by ephemeral per-process secrets. The new `pnpr cache-gc` command reclaims cache storage on demand with support for dry runs, custom size caps, age limits, and orphan-only cleanup [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
