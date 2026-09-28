## 1100.0.34

### Patch Changes

- Packages in an external `virtualStoreDir` can resolve the project's direct dependencies selected by `hoistPattern`. Run `pnpm install --force` to repair an existing installation [#5652](https://github.com/pnpm/pnpm/issues/5652).

- Fixed concurrent installs failing when replacing the same stale hoisted dependency link.

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/fs.symlink-dependency@1100.0.22
