---
"pacquet": patch
---

`pnpm import` now reads the versions recorded in `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock` and keeps them in the generated `pnpm-lock.yaml`. It resolved every dependency from scratch before, so the imported lockfile could pin newer versions than the one it was generated from. `pnpm import` also fails with `ERR_PNPM_LOCKFILE_NOT_FOUND` when none of those files are present [#14476](https://github.com/pnpm/pnpm/issues/14476).
