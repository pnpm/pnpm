---
"pacquet": patch
---

`pnpm install` no longer puts a dependency's bin on `PATH` for that dependency's own lifecycle scripts before the bin's file exists. pnpm links such a bin after the dependency's build has run. It also removes such a bin left by an earlier install. This fixes installing the `node` package on Windows [pnpm/pnpm#15501](https://github.com/pnpm/pnpm/issues/15501).
