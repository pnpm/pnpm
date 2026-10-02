---
"pacquet": patch
---

`pnpm install` no longer fails when a dependency of an optional dependency is missing from the registry. Like npm, pnpm now leaves out the nearest optional dependency above it, together with its subtree [#16511](https://github.com/pnpm/pnpm/issues/16511).
