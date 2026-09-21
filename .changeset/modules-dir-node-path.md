---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/building.after-install": patch
"@pnpm/exec.commands": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for the scripts and commands pnpm runs for a project. A symlinked tool started directly from a shell does not get it. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).
