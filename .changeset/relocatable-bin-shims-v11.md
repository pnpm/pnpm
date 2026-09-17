---
"pnpm": patch
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/building.after-install": patch
"@pnpm/building.during-install": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.hoist": patch
"@pnpm/workspace.injected-deps-syncer": patch
---

On macOS and Linux, project commands in `node_modules/.bin` now keep working after the project directory is moved or copied [#6937](https://github.com/pnpm/pnpm/issues/6937).
