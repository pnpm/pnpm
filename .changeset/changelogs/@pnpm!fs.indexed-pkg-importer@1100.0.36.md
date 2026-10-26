## 1100.0.36

### Patch Changes

- `pnpm deploy` now keeps a deployed symlink to a directory inside the project even if the directory is not among the deployed files, such as a link into `node_modules` [#17288](https://github.com/pnpm/pnpm/issues/17288).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/store.controller-types@1101.3.5
