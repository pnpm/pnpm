---
"pacquet": patch
---

`pnpm store prune` now removes the package metadata cache, the `metadata*` directories in the cache directory. This clears the outdated integrity checksums that cause `ERR_PNPM_TARBALL_INTEGRITY` when a registry republishes a version.
