---
"@pnpm/plugin-commands-audit": patch
"pnpm": patch
---

`pnpm audit --fix` should not add an override for a vulnerable package that has no fixes released.
