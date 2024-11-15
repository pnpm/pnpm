---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Detection of circular peer dependencies should not crash with aliased dependencies [#8759](https://github.com/pnpm/pnpm/issues/8759). Fixes a regression introduced in the previous version.
