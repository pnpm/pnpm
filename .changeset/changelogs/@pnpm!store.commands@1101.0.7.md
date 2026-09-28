## 1101.0.7

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.19
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/crypto.integrity@1100.0.8
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/global.packages@1101.1.5
  - @pnpm/installing.client@1100.3.12
  - @pnpm/installing.context@1101.0.7
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.connection-manager@1101.3.0
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/store.index@1100.3.3
  - @pnpm/store.path@1100.1.0
