---
"pacquet": patch
---

`pnpm install` now checks the store's files only for the packages it links into `node_modules`. A warm restore of a workspace using the global virtual store no longer stats every file of every package in the lockfile before skipping the slots that already exist [#14540](https://github.com/pnpm/pnpm/issues/14540).
