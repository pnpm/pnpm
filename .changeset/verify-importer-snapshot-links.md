---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` when an importer references a dependency version that has no `snapshots:` entry. The install used to succeed and leave a `node_modules` symlink pointing at a virtual-store directory that was never created [pnpm/pnpm#14764](https://github.com/pnpm/pnpm/issues/14764).
