## 1101.1.1

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/error@1100.2.1
  - @pnpm/workspace.project-manifest-reader@1100.1.1
