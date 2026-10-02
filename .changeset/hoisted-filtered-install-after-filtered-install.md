---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

With `nodeLinker: hoisted`, a filtered install of a workspace project no longer fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` after a filtered install of another project.
