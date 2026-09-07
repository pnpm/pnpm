---
"pacquet": patch
---

`pnpm deploy` now runs the source workspace's pnpmfile, not the copy of it that the deployed project's files leave in the deploy directory. A project that ships its own `.pnpmfile.mjs` no longer fails the deploy with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#14671](https://github.com/pnpm/pnpm/issues/14671).
