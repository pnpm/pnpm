---
"pacquet": patch
---

`pnpm store prune` now removes expired `pnpm dlx` cache entries. Prepare directories that a newer `pnpm dlx` run superseded no longer accumulate under the cache directory once they outlive `dlxCacheMaxAge` [#15171](https://github.com/pnpm/pnpm/issues/15171).
