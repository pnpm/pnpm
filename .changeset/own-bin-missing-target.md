---
"pacquet": patch
---

`pnpm install` no longer links a package's bin into that package's own `node_modules/.bin` while the bin's file does not exist, and removes such a bin left by an earlier install. This fixes installing the `node` package on Windows [pnpm/pnpm#15501](https://github.com/pnpm/pnpm/issues/15501).
