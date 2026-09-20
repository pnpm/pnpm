---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install --force` now reinstalls dependencies when the manifest and lockfile are unchanged. It previously reported "Already up to date" and left replaced dependency files in `node_modules` untouched [#919](https://github.com/pnpm/pnpm/issues/919).
