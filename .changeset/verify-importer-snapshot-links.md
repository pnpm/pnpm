---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` when an importer references a dependency version that has no snapshot entry. Previously the install succeeded and left a `node_modules` symlink pointing at a missing virtual-store directory [pnpm/pnpm#14764](https://github.com/pnpm/pnpm/issues/14764).
