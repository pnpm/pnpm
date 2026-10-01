---
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
---

`pnpm list` now shows an unsaved dependency named `constructor`, and `pnpm why` no longer labels a dependency aliased `constructor` as a dev dependency.
