---
"pacquet": patch
---

`pnpm deploy` now runs the source workspace's pnpmfile. It used to run the copy of that pnpmfile that the deploy leaves in the target directory. That failed the deploy with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` for any project that ships a `.pnpmfile.mjs` [#14671](https://github.com/pnpm/pnpm/issues/14671).
