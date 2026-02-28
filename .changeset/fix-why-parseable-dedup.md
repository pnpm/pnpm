---
"@pnpm/list": patch
"pnpm": patch
---

Fix `pnpm why -r --parseable` missing dependents when multiple workspace packages share the same dependency [#8100](https://github.com/pnpm/pnpm/issues/8100).
