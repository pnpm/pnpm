---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when `pnpm-lock.yaml` lists a workspace project whose directory or `package.json` is missing. The install used to report success without installing that project's dependencies [#7667](https://github.com/pnpm/pnpm/issues/7667).
