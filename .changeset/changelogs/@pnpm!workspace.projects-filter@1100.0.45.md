## 1100.0.45

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.9
  - @pnpm/error@1100.2.1
  - @pnpm/workspace.projects-graph@1100.0.40
  - @pnpm/workspace.projects-reader@1101.1.1
  - @pnpm/workspace.workspace-manifest-reader@1100.2.1
