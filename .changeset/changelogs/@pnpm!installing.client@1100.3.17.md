## 1100.3.17

### Patch Changes

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

- Updated dependencies:
  - @pnpm/engine.runtime.node-resolver@1101.3.9
  - @pnpm/fetching.binary-fetcher@1102.0.21
  - @pnpm/fetching.directory-fetcher@1100.0.40
  - @pnpm/fetching.git-fetcher@1102.0.26
  - @pnpm/fetching.tarball-fetcher@1102.1.10
  - @pnpm/hooks.types@1101.0.6
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/resolving.default-resolver@1101.0.12
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/store.index@1101.0.3
  - @pnpm/types@1102.1.2
