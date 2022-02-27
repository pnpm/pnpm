---
"@pnpm/core": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

In order to guarantee that only correct data is written to the store, data from the lockfile should not be written to the store. Only data directly from the package tarball or package metadata.
