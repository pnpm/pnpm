---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now ship a file that the `files` field names even when another entry excludes the directory holding it. For example, `["**", "!dist", "dist/index.d.ts"]` ships `dist/index.d.ts` [#16213](https://github.com/pnpm/pnpm/issues/16213).
