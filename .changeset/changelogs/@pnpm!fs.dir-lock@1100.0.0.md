## 1100.0.0

### Patch Changes

- Published the cross-process directory lock as `@pnpm/fs.dir-lock` [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
