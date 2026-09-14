## 1101.3.0

### Minor Changes

- Added a new setting `trustPolicyExcludePrune` (default: `false`). When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `trustPolicyExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

### Patch Changes

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.4
  - @pnpm/building.policy@1100.1.1
  - @pnpm/cli.utils@1101.0.27
  - @pnpm/config.reader@1102.2.0
  - @pnpm/config.writer@1100.0.26
  - @pnpm/deps.github-actions@1100.1.9
  - @pnpm/deps.inspection.outdated@1100.1.30
  - @pnpm/deps.path@1101.0.2
  - @pnpm/deps.security.signatures@1102.0.3
  - @pnpm/deps.status@1100.1.22
  - @pnpm/fs.graceful-fs@1100.2.1
  - @pnpm/global.commands@1102.0.1
  - @pnpm/global.packages@1101.1.2
  - @pnpm/hooks.pnpmfile@1100.0.31
  - @pnpm/installing.context@1101.0.4
  - @pnpm/installing.deps-installer@1104.1.2
  - @pnpm/installing.env-installer@1103.0.4
  - @pnpm/lockfile.fs@1100.2.7
  - @pnpm/lockfile.utils@1102.1.2
  - @pnpm/network.fetch@1100.1.16
  - @pnpm/pkg-manifest.utils@1100.4.4
  - @pnpm/resolving.npm-resolver@1104.1.2
  - @pnpm/store.connection-manager@1101.1.2
  - @pnpm/store.controller@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.0.28
  - @pnpm/workspace.projects-filter@1100.0.42
  - @pnpm/workspace.projects-graph@1100.0.37
  - @pnpm/workspace.projects-reader@1101.0.27
  - @pnpm/workspace.state@1100.0.43
  - @pnpm/workspace.workspace-manifest-writer@1100.2.0
