## 1100.0.33

### Patch Changes

- `pnpm licenses list` now reports the runtime downloaded through `devEngines.runtime` with `onFail: "download"`. The command previously failed with `ERR_PNPM_UNSUPPORTED_PACKAGE_TYPE` [#14172](https://github.com/pnpm/pnpm/issues/14172).

- Updated dependencies:
  - @pnpm/fetching.directory-fetcher@1100.0.33
  - @pnpm/store.cafs@1100.3.2
