---
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

readPackage hooks should not modify the `package.json` files in a workspace [#5670](https://github.com/pnpm/pnpm/issues/5670).
