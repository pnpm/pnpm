## 1100.0.34

### Patch Changes

- An async `updateConfig` hook that resolves to `undefined` now fails with `ERR_PNPM_CONFIG_IS_UNDEFINED`, as a synchronous hook that returns `undefined` already did.

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/error@1100.2.1
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/store.controller-types@1101.3.2
