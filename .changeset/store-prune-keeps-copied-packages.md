---
"pacquet": patch
---

`pnpm store prune` now keeps the packages that a project still uses when they were copied or cloned into `node_modules`. These include patched packages, built packages, and every package installed with `packageImportMethod: copy`. Before, the prune removed them, and the next install downloaded them again [#7192](https://github.com/pnpm/pnpm/issues/7192).
