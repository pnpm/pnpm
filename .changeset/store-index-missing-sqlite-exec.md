---
"@pnpm/store.index": patch
"pnpm": patch
---

`pnpm install` no longer fails with "this.db.exec is not a function" when `node:sqlite` lacks `DatabaseSync.exec`, as in StackBlitz WebContainers. When `node:sqlite` cannot prepare statements either, pnpm stores the index in `index.fallback` [#15649](https://github.com/pnpm/pnpm/issues/15649).
