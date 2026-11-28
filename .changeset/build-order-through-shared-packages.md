---
"pnpm": patch
---

A package's lifecycle scripts now always run after those of the dependencies it reaches. They could run at the same time when the dependency was reached only through a package without scripts that another dependency had reached first.
