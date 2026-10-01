---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` for a workspace project that declares `dependenciesMeta` and whose dependencies are all workspace links. pnpm now records that project's `dependenciesMeta` in `pnpm-lock.yaml` [#16457](https://github.com/pnpm/pnpm/issues/16457).
