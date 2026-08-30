## 1103.0.2

### Patch Changes

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/building.pkg-requires-build@1100.0.16
  - @pnpm/building.policy@1100.0.21
  - @pnpm/config.normalize-registries@1101.0.1
  - @pnpm/config.reader@1102.1.0
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/exec.lifecycle@1100.1.15
  - @pnpm/fs.symlink-dependency@1100.0.19
  - @pnpm/installing.context@1101.0.2
  - @pnpm/installing.modules-yaml@1101.0.1
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/lockfile.walker@1100.0.21
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/store.cafs@1100.3.0
  - @pnpm/store.connection-manager@1101.1.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/store.index@1100.3.0
  - @pnpm/types@1102.1.0
