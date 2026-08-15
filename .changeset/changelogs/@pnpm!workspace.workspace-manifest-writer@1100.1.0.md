## 1100.1.0

### Minor Changes

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

### Patch Changes

- `pnpm config delete <key>` no longer fails with `ENOENT` when the config file it would edit does not exist. Clearing a setting that was never set is a no-op [#13651](https://github.com/pnpm/pnpm/issues/13651).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.3
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/workspace.workspace-manifest-reader@1100.1.6
