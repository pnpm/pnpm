## 1100.2.0

### Minor Changes

- Added a new setting `trustPolicyExcludePrune` (default: `false`). When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `trustPolicyExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

### Patch Changes

- Updated dependencies:
  - @pnpm/building.policy@1100.1.1
