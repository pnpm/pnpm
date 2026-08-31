---
"pacquet": patch
---

Speed up workspace project discovery in large monorepos: workspace patterns are now probed concurrently and the discovered projects' `package.json` files are read in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).
