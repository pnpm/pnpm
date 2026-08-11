---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm update <pkg>@<version>` no longer re-resolves the entire dependency graph. Previously, any versioned selector disabled the per-package match predicate that scopes transitive updates, so unrelated transitive dependencies were re-resolved as well — every package floating under an in-range spec snapped to its latest eligible version on each update. Bare-name selectors already targeted correctly; pacquet already behaved correctly.
