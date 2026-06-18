---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed a lockfile non-convergence bug where an incremental install kept a duplicate transitive dependency that a fresh install would not produce. When a package is reused from the lockfile, its child edges are taken verbatim and bypass the preferred-versions walk, so a transitive dependency could stay pinned to an older version even after a direct dependency resolved to a higher version that satisfies the same range. The resolver now refreshes such a stale pin to the higher direct-dependency version during resolution — so the older version is never resolved or fetched, and the incremental result converges to the fresh one.
