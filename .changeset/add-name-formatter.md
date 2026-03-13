---
"@pnpm/reviewing.dependencies-hierarchy": minor
"@pnpm/list": patch
---

Added `nameFormatter` option to `buildDependentsTree` and `displayName` field to `DependentsTree`/`DependentNode`, allowing consumers to customize the displayed package name (e.g. showing component names instead of registry names).
