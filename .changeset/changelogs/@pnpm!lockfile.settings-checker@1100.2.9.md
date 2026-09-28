## 1100.2.9

### Patch Changes

- `pnpm install` now fails with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist. It used to fail with a raw `ENOENT` error and a stack trace [#5268](https://github.com/pnpm/pnpm/issues/5268).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.7
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/error@1100.2.1
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.verification@1100.1.8
