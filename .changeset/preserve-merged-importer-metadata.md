---
"@pnpm/lockfile.merger": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a project sets `publishConfig.directory`, `publishConfig.linkDirectory`, or `dependenciesMeta`. Lockfile merging now preserves and merges `publishDirectory`, `linkDirectory`, and `dependenciesMeta` on importers.
