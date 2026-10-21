## 1101.3.5

### Patch Changes

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.3
  - @pnpm/config.normalize-registries@1101.0.4
  - @pnpm/config.reader@1102.3.6
  - @pnpm/installing.client@1100.3.17
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.controller@1102.2.6
  - @pnpm/store.index@1101.0.3
  - @pnpm/store.path@1100.1.3
  - @pnpm/types@1102.1.2
