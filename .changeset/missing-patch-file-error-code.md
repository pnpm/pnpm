---
"pacquet": patch
---

`pnpm install` and `pnpm fetch` now fail with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist [#5268](https://github.com/pnpm/pnpm/issues/5268).
