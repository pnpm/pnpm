---
"@pnpm/building.commands": patch
"@pnpm/cli.default-reporter": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/building.during-install": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` should build any dependencies that were added to `onlyBuiltDependencies` and were not built yet [#10256](https://github.com/pnpm/pnpm/pull/10256).
