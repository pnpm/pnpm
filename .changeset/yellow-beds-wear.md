---
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/default-reporter": patch
"@pnpm/headless": patch
"@pnpm/build-modules": patch
"@pnpm/core": patch
"pnpm": patch
---

`pnpm install` should build any dependencies that were added to `onlyBuiltDependencies` and were not built yet [#10256](https://github.com/pnpm/pnpm/pull/10256).
