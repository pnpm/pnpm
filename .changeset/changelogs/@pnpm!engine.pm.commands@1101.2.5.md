## 1101.2.5

### Patch Changes

- `pnpm self-update` now checks that the version it installed can run before making it the active pnpm. A release that installs but cannot execute is discarded with an error instead of replacing a working installation.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.19
  - @pnpm/cli.utils@1101.0.16
  - @pnpm/config.reader@1101.12.2
  - @pnpm/global.commands@1100.0.36
  - @pnpm/installing.client@1100.2.16
  - @pnpm/installing.deps-restorer@1102.1.7
  - @pnpm/installing.env-installer@1102.0.8
  - @pnpm/lockfile.fs@1100.1.12
  - @pnpm/resolving.npm-resolver@1102.1.5
  - @pnpm/store.connection-manager@1100.3.8
  - @pnpm/workspace.project-manifest-reader@1100.0.17
