---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now writes plain versions for registry dependencies with peer dependencies in the deployed `package.json`. The deployed lockfile retains the resolved peer bindings. npm aliases keep their target package names [#14873](https://github.com/pnpm/pnpm/issues/14873).
