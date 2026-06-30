---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Hardened `pnpm deploy --force` so it refuses unsafe deploy targets such as workspace roots, parent directories, out-of-workspace paths, and symlinked target parents.
