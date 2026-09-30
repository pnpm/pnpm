---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add constructor` now adds the `constructor` package like any other dependency. pnpm used to read built-in object properties as its previous specifier and dependency type.
