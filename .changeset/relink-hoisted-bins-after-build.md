---
"@pnpm/building.during-install": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now relinks the bins in `node_modules/.pnpm/node_modules/.bin` after a dependency's build script replaces them, such as esbuild swapping its JavaScript launcher for a native binary. The hoisted `esbuild` command used to run the binary with `node`, which broke the build scripts of packages that depend on esbuild indirectly [#17402](https://github.com/pnpm/pnpm/issues/17402).
