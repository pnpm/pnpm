---
"@pnpm/plugin-commands-audit": patch
---

Fix `pnpm audit --fix` replacing reference overrides (e.g. `$foo`) with concrete versions.
