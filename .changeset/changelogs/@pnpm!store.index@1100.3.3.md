## 1100.3.3

### Patch Changes

- `pnpm install` keeps the owner, group, and mode of files already in a shared store, including `index.db`. New store files and directories inherit the store directory's group-write bit. When that directory is setgid, new files inherit its group. pnpm does not change a file's owner or group [pnpm/pnpm#12765](https://github.com/pnpm/pnpm/issues/12765).

- `pnpm install` no longer fails with "this.db.exec is not a function" when `node:sqlite` lacks `DatabaseSync.exec`, as in StackBlitz WebContainers. When `node:sqlite` cannot prepare statements either, pnpm stores the index in `index.fallback` [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
