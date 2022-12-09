---
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

`pnpm update` should not replace `workspace:*`, `workspace:~`, and `workspace:^` with `workspace:<version>` [#5764](https://github.com/pnpm/pnpm/pull/5764).
