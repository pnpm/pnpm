---
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now reads the same local tarball it installs when a dependency's absolute `file:` path contains `..`. Such a path could install a different tarball than the one it read, failing with `ERR_PNPM_TARBALL_INTEGRITY`, or fail to resolve at all.
