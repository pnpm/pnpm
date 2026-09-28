## 1101.0.39

### Patch Changes

- `pnpm audit` and `pnpm audit signatures` now fail with an error when the lockfile contains unresolvable dependency references [pnpm/pnpm#13638](https://github.com/pnpm/pnpm/issues/13638).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/lockfile.detect-dep-types@1100.0.26
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.peer-edges@1100.0.1
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/lockfile.walker@1100.0.26
  - @pnpm/network.fetch@1100.1.19
