## 1100.0.41

### Patch Changes

- Fixed `ERR_PNPM_LINKING_FAILED` on filesystems without copy-on-write reflinks when an install reuses an existing global virtual store entry before staging a package build. Also fixed merging `node_modules` directories during staged package imports on filesystems such as Btrfs that report `EEXIST` when renaming onto a non-empty directory.

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/error@1100.2.3
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/store.controller-types@1101.3.8
