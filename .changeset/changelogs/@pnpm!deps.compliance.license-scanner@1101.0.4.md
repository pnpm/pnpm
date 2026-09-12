## 1101.0.4

### Patch Changes

- `pnpm licenses list` now reports the runtime downloaded through `devEngines.runtime` with `onFail: "download"`. The command previously failed with `ERR_PNPM_UNSUPPORTED_PACKAGE_TYPE` [#14172](https://github.com/pnpm/pnpm/issues/14172).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.2
  - @pnpm/lockfile.detect-dep-types@1100.0.23
  - @pnpm/lockfile.fs@1100.2.7
  - @pnpm/lockfile.utils@1102.1.2
  - @pnpm/lockfile.walker@1100.0.23
  - @pnpm/store.pkg-finder@1100.0.33
