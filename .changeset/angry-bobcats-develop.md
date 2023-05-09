---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm link -g <pkg-name>` should not modify the `package.json` file [#4341](https://github.com/pnpm/pnpm/issues/4341).
