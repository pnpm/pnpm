---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --ignore-pnpmfile` and `pnpm dedupe --ignore-pnpmfile` now keep the `pnpmfileChecksum` recorded in `pnpm-lock.yaml`. `pnpm install --frozen-lockfile --ignore-pnpmfile` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#10944](https://github.com/pnpm/pnpm/issues/10944).
