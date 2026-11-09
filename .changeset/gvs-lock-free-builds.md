---
"@pnpm/building.after-install": patch
"@pnpm/building.during-install": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.graph-hasher": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/store.commands": patch
"@pnpm/store.controller": patch
"pnpm": patch
"pacquet": patch
---

Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).
