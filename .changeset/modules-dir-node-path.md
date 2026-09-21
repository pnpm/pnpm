---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/building.after-install": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

Command shims in a custom `modulesDir` now add that directory to `NODE_PATH`, so tools such as ESLint can load plugins installed there. The directory comes after the tool's own dependencies, and `extendNodePath: false` leaves it out [#3604](https://github.com/pnpm/pnpm/issues/3604).
