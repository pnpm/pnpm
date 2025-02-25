---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm link <path>` should calculate relative path from the root of the workspace directory [#9132](https://github.com/pnpm/pnpm/pull/9132).
