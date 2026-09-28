## 1100.1.12

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/error@1100.2.1
  - @pnpm/network.git-utils@1100.0.6
  - @pnpm/resolving.git-resolver@1100.1.24
