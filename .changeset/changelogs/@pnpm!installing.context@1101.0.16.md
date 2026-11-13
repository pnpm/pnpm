## 1101.0.16

### Patch Changes

- New workspace projects now prefer peer versions already pinned in the lockfile. Dependencies that are also optional peers no longer cause an unrelated project to select an older peer version.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.42
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/installing.read-projects-context@1101.0.15
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.pruner@1100.0.30
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.controller@1102.2.10
