---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm dlx` should work with git-hosted packages. For example: `pnpm dlx gengjiawen/envinfo` [#4714](https://github.com/pnpm/pnpm/issues/4714).
