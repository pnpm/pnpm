## 1100.15.0

### Minor Changes

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

### Patch Changes

- `pnpm prune` is now recursive by default inside a workspace, just like `pnpm install`. This fixes `pnpm prune --prod` in a workspace root emptying the `node_modules` directories of the other workspace projects, dropping the links to the workspace packages they depend on in production [#13718](https://github.com/pnpm/pnpm/issues/13718).

- Updated dependencies:
  - @pnpm/building.after-install@1102.0.17
  - @pnpm/building.policy@1100.0.19
  - @pnpm/catalogs.config@1100.0.5
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/config.writer@1100.0.22
  - @pnpm/deps.github-actions@1100.1.5
  - @pnpm/deps.inspection.outdated@1100.1.25
  - @pnpm/deps.security.signatures@1101.3.1
  - @pnpm/deps.status@1100.1.17
  - @pnpm/error@1100.1.2
  - @pnpm/global.commands@1100.1.1
  - @pnpm/global.packages@1100.0.18
  - @pnpm/hooks.pnpmfile@1100.0.27
  - @pnpm/installing.context@1100.1.2
  - @pnpm/installing.dedupe.check@1100.1.9
  - @pnpm/installing.deps-installer@1103.2.0
  - @pnpm/installing.env-installer@1102.0.17
  - @pnpm/lockfile.fs@1100.2.2
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/network.auth-header@1101.1.10
  - @pnpm/network.fetch@1100.1.12
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/pkg-manifest.utils@1100.4.0
  - @pnpm/resolving.npm-resolver@1103.2.1
  - @pnpm/store.connection-manager@1100.3.17
  - @pnpm/store.controller@1102.0.12
  - @pnpm/workspace.project-manifest-reader@1100.0.24
  - @pnpm/workspace.projects-filter@1100.0.37
  - @pnpm/workspace.projects-graph@1100.0.33
  - @pnpm/workspace.projects-reader@1101.0.23
  - @pnpm/workspace.root-finder@1100.0.6
  - @pnpm/workspace.state@1100.0.38
  - @pnpm/workspace.workspace-manifest-reader@1100.1.6
  - @pnpm/workspace.workspace-manifest-writer@1100.1.0
