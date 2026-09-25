---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now copies the `packageManager` and `devEngines.packageManager` fields of the workspace root `package.json` into the deployed `package.json`, unless the deployed project pins a package manager itself [#9079](https://github.com/pnpm/pnpm/issues/9079).
