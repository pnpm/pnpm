---
"pacquet": patch
---

A hoisted-linker install no longer fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` when an optional dependency's snapshot is absent because it was skipped on a previous install.
