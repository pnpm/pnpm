## 1100.0.22

### Patch Changes

- `pnpm install` no longer creates a `node_modules` symlink inside the `publishConfig.directory` of a workspace package linked with `linkDirectory`. A build tool that cleaned its output directory through that symlink deleted the files of the package's dependencies. `pnpm install` also removes a symlink that an earlier install left there [#16226](https://github.com/pnpm/pnpm/issues/16226).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/fs.symlink-dependency@1100.0.22
