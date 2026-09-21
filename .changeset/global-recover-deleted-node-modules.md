---
"pnpm": patch
"pacquet": patch
---

`pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` now recover a global package group whose entire `node_modules` directory was deleted. pnpm can no longer tell which commands such a group installed, so `pnpm remove -g` may leave one of its shims in the global bin directory [#15093](https://github.com/pnpm/pnpm/issues/15093).
