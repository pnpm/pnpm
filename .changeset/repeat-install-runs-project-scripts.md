---
"pacquet": patch
---

With `optimisticRepeatInstall: false`, `pnpm install` now runs the projects' own lifecycle scripts, such as `prepare`, even when `node_modules` is already up to date [#16545](https://github.com/pnpm/pnpm/issues/16545).
