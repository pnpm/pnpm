---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now ship a file that the `files` field names inside a directory the same field excludes. A package listing `files: ["**", "!dist", "dist/index.d.ts"]` publishes that declaration file again [#16213](https://github.com/pnpm/pnpm/issues/16213).
