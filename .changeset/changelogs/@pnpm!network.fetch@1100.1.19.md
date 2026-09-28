## 1100.1.19

### Patch Changes

- `pnpm install` now reports a full content-addressable store without retrying the tarball when writing package files fails. Related to [pnpm/pnpm#8581](https://github.com/pnpm/pnpm/issues/8581).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
