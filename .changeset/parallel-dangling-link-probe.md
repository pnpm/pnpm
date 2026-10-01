---
"pacquet": patch
---

A repeat `pnpm install` in a large workspace reports "Already up to date" faster. pnpm now checks the direct dependency links of the workspace projects in parallel [#16487](https://github.com/pnpm/pnpm/issues/16487).
