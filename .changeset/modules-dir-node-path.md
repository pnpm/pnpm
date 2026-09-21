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

Tools launched from a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).
