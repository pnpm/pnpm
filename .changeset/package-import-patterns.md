---
"pacquet": minor
---

Added `packageImportPatterns`. When it is set, `pnpm install` imports into `node_modules` only the package files whose path matches one of its patterns, and each package's `package.json`. An install that only a type checker reads can list `*.d.ts`, `*.ts` and `*.json`, and link a fraction of every package. The patterns are those of `publicHoistPattern`: `*` matches any characters, `/` included, and a leading `!` excludes. The setting cannot be combined with the global virtual store, whose package directories every project on the machine shares.
