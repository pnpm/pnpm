## 1101.0.14

### Patch Changes

- Commands installed with `pnpm add --global` now find the dependencies that pnpm hoists for their package. A package that loads a dependency it does not declare, such as a native addon's platform-specific package, failed with "Cannot find module". Reinstall such a package with `pnpm add --global` to update its existing command [#17503](https://github.com/pnpm/pnpm/issues/17503).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/installing.read-projects-context@1101.0.13
  - @pnpm/lockfile.fs@1100.2.16
  - @pnpm/store.controller@1102.2.8
