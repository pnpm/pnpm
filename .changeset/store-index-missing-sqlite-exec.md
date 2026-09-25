---
"@pnpm/store.index": patch
"pnpm": patch
---

`pnpm install` works when `node:sqlite` `DatabaseSync.exec` is missing. pnpm runs the store index SQL through prepared statements when that method is absent. When the host cannot prepare statements either, pnpm stores the index in `index.fallback` [#15649](https://github.com/pnpm/pnpm/issues/15649).
