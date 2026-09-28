## 1103.0.7

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/config.writer@1100.0.29
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/installing.deps-resolver@1102.2.4
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.pruner@1100.0.26
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/resolving.tarball-url@1101.1.3
  - @pnpm/store.controller@1102.2.1
  - @pnpm/store.controller-types@1101.3.2
