## 1100.0.30

### Patch Changes

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a project sets or removes `publishConfig.directory`, `publishConfig.linkDirectory`, or `dependenciesMeta`. Lockfile merging now preserves and reconciles `publishDirectory`, `linkDirectory`, and `dependenciesMeta` on importers.

- Updated dependencies:
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/types@1102.1.4
