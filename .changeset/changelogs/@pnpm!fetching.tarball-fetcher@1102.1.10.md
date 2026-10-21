## 1102.1.10

### Patch Changes

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.3
  - @pnpm/exec.prepare-package@1100.0.44
  - @pnpm/fetching.fetcher-base@1100.2.13
  - @pnpm/fs.packlist@1100.0.9
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/store.cafs@1100.3.8
  - @pnpm/store.index@1101.0.3
  - @pnpm/types@1102.1.2
