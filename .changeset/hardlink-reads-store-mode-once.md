---
"pacquet": patch
---

`pnpm install` makes fewer `stat` calls when it hardlinks files from a hot store. It reuses the file modes that the store integrity check has already read. With the default `verifyStoreIntegrity`, an offline install makes as many `stat` calls as 12.7.0 did [pnpm/pnpm#17543](https://github.com/pnpm/pnpm/issues/17543).
