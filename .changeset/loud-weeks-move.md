---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

`pnpm deploy` should inject local dependencies of all types (dependencies, optionalDependencies, devDependencies) [#5078](https://github.com/pnpm/pnpm/issues/5078).
