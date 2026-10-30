---
"@pnpm/worker": patch
"@pnpm/store.create-cafs-store": patch
"pnpm": patch
---

A package with `bundledDependencies` and a build script now keeps its bundled dependencies when it is restored from the side-effects cache [pnpm/pnpm#17523](https://github.com/pnpm/pnpm/issues/17523).
