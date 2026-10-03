## 1103.0.9

### Patch Changes

- `pnpm rebuild` and `pnpm approve-builds` refresh command launchers when a build changes a command's interpreter or replaces it with a native executable.

  Dependent packages' build scripts use the refreshed launchers.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.36
  - @pnpm/building.policy@1100.1.5
  - @pnpm/config.reader@1102.3.3
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/exec.lifecycle@1100.1.22
  - @pnpm/fs.symlink-dependency@1100.0.23
  - @pnpm/installing.context@1101.0.9
  - @pnpm/installing.deps-restorer@1103.2.3
  - @pnpm/lockfile.settings-checker@1100.2.11
  - @pnpm/store.cafs@1100.3.6
  - @pnpm/store.connection-manager@1101.3.2
  - @pnpm/store.controller-types@1101.3.3
