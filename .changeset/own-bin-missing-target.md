---
"pacquet": patch
---

`pnpm install` no longer links a dependency's bin while the bin's file does not exist and the dependency's lifecycle scripts have yet to run. pnpm links the bin after those scripts create it. It also removes such a bin left by an earlier install. This fixes installing the `node` package on Windows [pnpm/pnpm#15501](https://github.com/pnpm/pnpm/issues/15501).
