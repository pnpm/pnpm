---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install --force` now runs a full install, so it repairs dependency files that were modified inside `node_modules`. It reported "Already up to date" and changed nothing when the manifest and the lockfile were unchanged [#919](https://github.com/pnpm/pnpm/issues/919).
