---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"@pnpm/constants": patch
"pacquet": patch
"pnpm": patch
---

Dependency resolution loads cached registry metadata faster using an indexed on-disk layout. The cache is stored under `<cache-dir>/v12/`. Damaged cache entries are refetched, or reported with an error when `--offline` is set [#13512](https://github.com/pnpm/pnpm/pull/13512).
