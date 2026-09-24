---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`--ignore-pnpmfile` now keeps the `pnpmfileChecksum` recorded in `pnpm-lock.yaml`. `pnpm install --ignore-pnpmfile` and `pnpm dedupe --ignore-pnpmfile` used to remove it, and `pnpm install --frozen-lockfile --ignore-pnpmfile` failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#10944](https://github.com/pnpm/pnpm/issues/10944).
