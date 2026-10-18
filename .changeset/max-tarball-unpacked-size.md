---
"@pnpm/store.cafs": patch
"@pnpm/worker": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/fetching.binary-fetcher": patch
"@pnpm/config.reader": patch
"@pnpm/installing.client": patch
"@pnpm/store.connection-manager": patch
"pnpm": patch
---

Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).
