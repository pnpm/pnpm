## 1101.1.3

### Patch Changes

- Fixed frozen installs creating symlinks to the working directory for skipped optional dependencies and unresolved peer dependencies [#16454](https://github.com/pnpm/pnpm/issues/16454).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.2
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/fs.symlink-dependency@1100.0.23
  - @pnpm/lockfile.fs@1100.2.12
  - @pnpm/store.controller-types@1101.3.3
