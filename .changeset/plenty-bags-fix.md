---
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/headless": patch
"@pnpm/core": patch
"@pnpm/lifecycle": patch
"pnpm": patch
---

Local workspace bin files that should be compiled first are linked to dependent projects after compilation [#1801](https://github.com/pnpm/pnpm/issues/1801).
