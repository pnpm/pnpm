---
"pnpm": patch
"pacquet": patch
---

`pnpm clean`, `pnpm purge`, and `pnpm ci` keep files outside the project untouched when `modulesDir` points outside the project root. When `node_modules` is a symlink, `pnpm` now unlinks the symlink without modifying its target [zkochan/pnpm-tasks#104](https://github.com/zkochan/pnpm-tasks/issues/104).
