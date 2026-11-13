---
"pnpm": patch
"pacquet": patch
---

`pnpm clean`, `pnpm purge`, and `pnpm ci` keep files outside the project untouched when `modulesDir` points outside the project root. When `node_modules` is a symlink, `pnpm` now unlinks the symlink without modifying its target [#18072](https://github.com/pnpm/pnpm/pull/18072).
