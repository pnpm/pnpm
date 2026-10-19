---
"pacquet": patch
---

`pnpm install` with an existing lockfile and a second `pnpm dedupe` now keep optional peer bindings that point into a dependency cycle or back into the dependent package. Before, the second run dropped the optional peer binding from the lockfile snapshot key.
