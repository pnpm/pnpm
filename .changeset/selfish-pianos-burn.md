---
"@pnpm/filter-workspace-packages": minor
"pnpm": patch
---

Fix a bug in which `use-node-version` or `node-version` isn't passed down to `checkEngine` when using pnpm workspace, resulting in an error [#6981](https://github.com/pnpm/pnpm/issues/6981).
