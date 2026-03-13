---
"@pnpm/plugin-commands-audit": patch
"pnpm": patch
---

Fix `pnpm audit --fix` replacing reference overrides (e.g. `$foo`) with concrete versions [#10325](https://github.com/pnpm/pnpm/issues/10325).
