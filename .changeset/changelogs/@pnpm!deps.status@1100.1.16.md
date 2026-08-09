## 1100.1.16

### Patch Changes

- Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.

- Updated dependencies:
  - @pnpm/config.reader@1101.16.1
  - @pnpm/installing.context@1100.1.1
  - @pnpm/lockfile.fs@1100.2.1
  - @pnpm/lockfile.settings-checker@1100.2.0
  - @pnpm/lockfile.verification@1100.0.32
  - @pnpm/workspace.state@1100.0.37
