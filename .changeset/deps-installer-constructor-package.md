---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` no longer crashes or writes a wrong lockfile when a dependency, peer dependency, or catalog entry is named `constructor`.
