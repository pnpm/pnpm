---
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pnpm": minor
---

Resolution reads far less registry metadata from the cache: the on-disk metadata mirror now uses the indexed layout the Rust CLI already writes (`pacquet-meta-v1`), so loading a packument parses only a small index and the manifests of the versions an install actually picks, instead of every version of every package. Mirrors written by older pnpm versions are still read, and mirrors written by either CLI flavor are now shared instead of being treated as cache misses. Resolving a 1246-package fixture from a warm cache got about 16% faster from this change alone.
