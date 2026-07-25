---
"pacquet": patch
---

Fixed `ERR_PNPM_BROKEN_LOCKFILE` when installing with a pnpm 10 lockfile that has a `patchedDependencies` section. See pnpm/pnpm#13307.
