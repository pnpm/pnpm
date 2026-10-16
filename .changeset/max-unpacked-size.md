---
"@pnpm/store.cafs": patch
"@pnpm/store.cafs-types": patch
"@pnpm/worker": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/config.reader": patch
"@pnpm/installing.client": patch
"@pnpm/store.connection-manager": patch
"pnpm": patch
"pacquet": minor
---

`pnpm install` now rejects a package whose files unpack to more than 2 GiB in total and fails with `ERR_PNPM_TARBALL_UNPACKED_SIZE_EXCEEDED`. A small archive can unpack to a far larger size and fill the disk. The new `maxUnpackedSize` setting changes the limit, in bytes [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).
