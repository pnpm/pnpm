---
"pacquet": minor
---

`pnpm install` now fails with `ERR_PNPM_UNUSED_PATCH` when an entry in `patchedDependencies` doesn't match any installed package. Set `allowUnusedPatches: true` in `pnpm-workspace.yaml` to get a warning instead, matching pnpm 11 [#11633](https://github.com/pnpm/pnpm/issues/11633).
