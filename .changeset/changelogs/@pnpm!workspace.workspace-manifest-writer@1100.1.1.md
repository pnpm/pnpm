## 1100.1.1

### Patch Changes

- `pnpm approve-builds` now removes `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml` when it writes `allowBuilds`. Those settings were replaced by `allowBuilds` in pnpm 11 and silently ignored since, so a workspace migrated from pnpm 10 kept them around looking active.

- `pnpm remove` now prunes undecided entries (`"set this to true or false"`) from `allowBuilds` in `pnpm-workspace.yaml` when `sharedWorkspaceLockfile: true` and the corresponding packages are removed [pnpm/pnpm#13892](https://github.com/pnpm/pnpm/issues/13892).

- Updated dependencies:
  - @pnpm/building.policy@1100.0.20
  - @pnpm/config.parse-overrides@1100.1.4
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/constants@1102.0.0
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.workspace-manifest-reader@1100.1.7
