---
"pacquet": patch
---

A repeated `pnpm install --filter` now finishes without reinstalling when the selected projects are up to date. So does `pnpm install` once filtered commands have installed every project.
