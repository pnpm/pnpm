---
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
---

`pnpm list --json` now reports unsaved dependencies from each workspace project's own modules directory [#18248](https://github.com/pnpm/pnpm/issues/18248).
