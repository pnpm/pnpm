---
"pacquet": patch
---

`pnpm pack` ships a file that the `files` field names even when another entry excludes the directory holding it, so `["**", "!dist", "dist/index.d.ts"]` no longer drops `dist/index.d.ts` [#16213](https://github.com/pnpm/pnpm/issues/16213).
