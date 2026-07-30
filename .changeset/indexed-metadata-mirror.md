---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pnpm": patch
---

Dependency resolution loads cached registry metadata faster: the on-disk metadata cache now uses the same indexed layout as the Rust-based pnpm CLI, and caches written by either CLI flavor are shared instead of refetched. Caches written by older pnpm versions are still read.
