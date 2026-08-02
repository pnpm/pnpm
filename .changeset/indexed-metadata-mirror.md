---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"@pnpm/constants": patch
"pacquet": patch
"pnpm": patch
---

Dependency resolution loads cached registry metadata faster: the on-disk metadata cache now uses an indexed layout that both the TypeScript and the Rust-based pnpm CLI read and write, so a cache written by either one is shared instead of refetched. The cache moved to `<cache-dir>/v12/`, because the format is not backwards compatible — the first install re-downloads metadata, and the old `v11` directory can be reclaimed with `pnpm store prune`. A cache entry that turns out to be damaged is refetched instead of being resolved from, and reports a clear error when `--offline` leaves nothing to refetch from.
