## 1100.0.25

### Patch Changes

- `pnpm install` now succeeds when `node_modules` is a symlink to an existing directory. Dependency hoisting previously rejected a symlinked `node_modules` as an invalid destination [pnpm/pnpm#17157](https://github.com/pnpm/pnpm/issues/17157).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/types@1102.1.2
