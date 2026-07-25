---
"pacquet": patch
---

Fixed `ERR_PNPM_BROKEN_LOCKFILE` on a lockfile written by pnpm 10 that has a `patchedDependencies` section. pnpm 10 records each patched dependency as a `{hash, path}` mapping, while newer versions record the bare patch-file hash. Both shapes are now read, so such a lockfile installs unchanged and is normalized the next time it is written. See pnpm/pnpm#13307.
