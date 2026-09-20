## 1102.0.2

### Patch Changes

- `pnpm add -g` and `pnpm update -g` now ignore incomplete unrelated global package groups when every command from the replaced group is retained. Operations that could remove a global command still require complete ownership information.

- `pnpm update --global` no longer reinstalls a global package when its dependency graph resolves to what is already installed. It reports `Already up to date` [pnpm/pnpm#12002](https://github.com/pnpm/pnpm/issues/12002).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.32
  - @pnpm/bins.remover@1100.0.24
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/deps.inspection.list@1101.0.5
  - @pnpm/global.packages@1101.1.3
  - @pnpm/installing.deps-installer@1104.1.3
  - @pnpm/installing.modules-yaml@1101.0.3
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/store.connection-manager@1101.1.3
