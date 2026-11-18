---
"pnpm": patch
---

`pnpm ci` no longer reports an existing current lockfile to the `preResolution` hook when `virtualStoreDir` points outside the project and the lockfile is about to be removed with `node_modules`.
