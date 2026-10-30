## 1102.0.11

### Patch Changes

- Commands installed with `pnpm add --global` now find the dependencies that pnpm hoists for their package. A package that loads a dependency it does not declare, such as a native addon's platform-specific package, failed with "Cannot find module". Reinstall such a package with `pnpm add --global` to update its existing command [#17503](https://github.com/pnpm/pnpm/issues/17503).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/config.reader@1102.3.8
  - @pnpm/deps.inspection.list@1101.1.2
  - @pnpm/installing.deps-installer@1104.2.8
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/store.connection-manager@1101.3.7
