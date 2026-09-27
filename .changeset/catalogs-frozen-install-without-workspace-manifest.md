---
"@pnpm/config.reader": patch
"@pnpm/lockfile.settings-checker": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now succeeds in a directory that has a lockfile with catalogs but no `pnpm-workspace.yaml`, such as a pruned production build. It failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` before. A `pnpm-workspace.yaml` that drops a catalog entry still fails the frozen install [#10551](https://github.com/pnpm/pnpm/issues/10551).
