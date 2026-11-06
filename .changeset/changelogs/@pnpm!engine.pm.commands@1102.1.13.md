## 1102.1.13

### Patch Changes

- `pnpm self-update` now reinstalls a cached engine if its executable cannot run. This repairs shared store entries damaged by an earlier install [pnpm/pnpm#17582](https://github.com/pnpm/pnpm/issues/17582).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.41
  - @pnpm/building.during-install@1102.2.12
  - @pnpm/config.pick-registry-for-package@1101.0.5
  - @pnpm/deps.graph-hasher@1100.3.11
  - @pnpm/global.commands@1102.0.12
  - @pnpm/global.packages@1101.1.10
  - @pnpm/installing.client@1100.3.20
  - @pnpm/installing.deps-restorer@1103.2.9
  - @pnpm/installing.env-installer@1103.0.15
  - @pnpm/lockfile.fs@1100.2.17
  - @pnpm/resolving.npm-resolver@1104.2.9
  - @pnpm/store.connection-manager@1101.3.8
  - @pnpm/store.controller@1102.2.9
