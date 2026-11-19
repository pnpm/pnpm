---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` no longer fails without a TTY when it has to recreate an incompatible `node_modules` directory, such as in a Docker build or with piped stdin. It now recreates the directory and logs a message. A custom or symlinked modules directory still requires setting `confirmModulesPurge` to `false` [#6778](https://github.com/pnpm/pnpm/issues/6778), [#9166](https://github.com/pnpm/pnpm/issues/9166), [#8654](https://github.com/pnpm/pnpm/issues/8654), [#8085](https://github.com/pnpm/pnpm/issues/8085).
