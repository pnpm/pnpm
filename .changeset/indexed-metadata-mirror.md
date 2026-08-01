---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pacquet": patch
"pnpm": patch
---

Dependency resolution loads cached registry metadata faster: the on-disk metadata cache now uses the same indexed layout as the Rust-based pnpm CLI, and caches written by either CLI flavor are shared instead of refetched. Caches written by older pnpm versions are still read. A cache entry that turns out to be damaged is now refetched instead of being resolved from, and reports a clear error when `--offline` leaves nothing to refetch from.
