---
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

The update command should not replace dependency versions specified via dist-tags [#5996](https://github.com/pnpm/pnpm/pull/5996).
