## 1101.1.5

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/error@1100.2.1
  - @pnpm/pkg-manifest.reader@1100.0.21
