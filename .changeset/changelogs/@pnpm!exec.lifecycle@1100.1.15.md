## 1100.1.15

### Patch Changes

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Fixed recursive `run` cleanup on Windows when a lifecycle script fails while another script's process tree is still running.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/fetching.directory-fetcher@1100.0.31
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/store.cafs-types@1100.1.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
