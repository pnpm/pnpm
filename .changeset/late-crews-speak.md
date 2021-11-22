---
"@pnpm/plugin-commands-setup": patch
"pnpm": patch
---

`pnpm setup` should create shell rc files for pnpm path configuration if no such file exists prior [#4027](https://github.com/pnpm/pnpm/issues/4027).
