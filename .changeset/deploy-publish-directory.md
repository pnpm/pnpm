---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` no longer fails when the deployed project depends on a workspace package that sets `publishConfig.directory`. pnpm installs that package from its publish directory and runs its build scripts [pnpm/pnpm#17771](https://github.com/pnpm/pnpm/pull/17771).
