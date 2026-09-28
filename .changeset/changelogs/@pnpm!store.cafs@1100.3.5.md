## 1100.3.5

### Patch Changes

- The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

  After upgrading, every package with a build script is built once more.

- `pnpm install` keeps the owner, group, and mode of files already in a shared store, including `index.db`. New store files and directories inherit the store directory's group-write bit. When that directory is setgid, new files inherit its group. pnpm does not change a file's owner or group [pnpm/pnpm#12765](https://github.com/pnpm/pnpm/issues/12765).

- pnpm now uses less memory when installing a package whose archive is larger than 64 MiB unpacked. It decompresses such archives as a stream [#14164](https://github.com/pnpm/pnpm/issues/14164).

- When `pnpm install` repairs a store file that was modified through a hard link in `node_modules`, the repair now keeps the file's inode on Linux and macOS, so hard-linked copies in other projects are healed at the same time. Previously, only the project running the install received the restored content. On Windows the repair still replaces the file, so other projects are healed on their next install [pnpm/pnpm#3445](https://github.com/pnpm/pnpm/issues/3445).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/store.controller-types@1101.3.2
