---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm import` no longer crashes when the imported lockfile or a Yarn patch refers to a package named `constructor`.
