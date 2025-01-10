---
"@pnpm/config": patch
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Fix infinite loop caused by lifecycle scripts using `pnpm` to execute other scripts during `pnpm install` with `verify-deps-before-run=install` [#8954](https://github.com/pnpm/pnpm/issues/8954).
