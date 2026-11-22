---
"pnpm": patch
---

`pnpm list -g` with `--depth` greater than `0` now always reads the global install directory's `node_modules` and lockfile. Previously it could read the current project's lockfile or a custom `modulesDir`.
