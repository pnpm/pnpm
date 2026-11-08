---
"pacquet": patch
---

On Windows, `packageImportMethod: auto` now tries hardlinking before cloning, as it does on Linux. Installs from a warm store on a Dev Drive are faster [#17809](https://github.com/pnpm/pnpm/issues/17809).
