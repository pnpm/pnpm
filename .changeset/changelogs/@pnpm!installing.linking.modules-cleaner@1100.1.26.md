## 1100.1.26

### Patch Changes

- Virtual store cleanup now preserves temporary lockfiles written by concurrent installs.

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.remover@1100.0.26
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/lockfile.filtering@1100.2.10
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/store.controller-types@1101.3.2
