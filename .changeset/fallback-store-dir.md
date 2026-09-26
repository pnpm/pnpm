---
"pacquet": minor
---

Added the `fallbackStoreDir` setting. It points at a read-only store that pnpm copies a package from when the package is missing from `storeDir`, before downloading it. pnpm checks every copied file against its recorded hash and never writes to the fallback store. The setting has no effect with `frozenStore` [#3392](https://github.com/pnpm/pnpm/issues/3392).
