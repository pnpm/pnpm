## 1101.3.5

### Patch Changes

- `pnpm deploy` now keeps a deployed symlink to a directory inside the project even if the directory is not among the deployed files, such as a link into `node_modules` [#17288](https://github.com/pnpm/pnpm/issues/17288).

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.14
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/types@1102.1.3
