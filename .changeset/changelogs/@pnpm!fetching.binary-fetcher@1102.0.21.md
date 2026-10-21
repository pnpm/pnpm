## 1102.0.21

### Patch Changes

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.13
  - @pnpm/store.cafs@1100.3.8
  - @pnpm/store.index@1101.0.3
