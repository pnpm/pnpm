---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` for a workspace project that declares `dependenciesMeta` and whose dependencies are all workspace links. The project's `dependenciesMeta` is now recorded in `pnpm-lock.yaml`.
