---
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm now reads the same local tarball it installs when a dependency's absolute `file:` path contains `..`. A `..` that stepped through a symlink used to read one tarball and install another, which failed with `ERR_PNPM_TARBALL_INTEGRITY`. A `..` that stepped back through a directory that does not exist used to fail to resolve at all.
