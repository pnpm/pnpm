---
"pacquet": patch
---

On Windows, `pnpm store path` now prints a path that uses backslashes throughout. The `storeDir` and `virtualStoreDir` values recorded in `node_modules/.modules.yaml` use backslashes as well. Both previously carried forward slashes in the middle of the path, so they did not match the values pnpm 11 writes for the same directory.
