## 1102.2.1

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/installing.package-requester@1102.2.1
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/store.create-cafs-store@1100.0.32
  - @pnpm/store.index@1100.3.3
