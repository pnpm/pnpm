## 1100.1.1

### Patch Changes

- Global installs now switch over atomically. The command shims in the global bin directory point at a stable per-package link rather than at the directory a particular install produced, so `pnpm add -g` and `pnpm update -g` activate a new version by moving that one link instead of rewriting every shim. A command can no longer be missing from `PATH` while an install is in progress, and a failed install leaves the previous version in place.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.27
  - @pnpm/bins.remover@1100.0.20
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/deps.inspection.list@1100.1.2
  - @pnpm/error@1100.1.2
  - @pnpm/global.packages@1100.0.18
  - @pnpm/installing.deps-installer@1103.2.0
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/pkg-manifest.utils@1100.4.0
  - @pnpm/store.connection-manager@1100.3.17
