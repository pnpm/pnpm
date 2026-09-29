---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` no longer copies the workspace root's `devEngines.packageManager` into the deployed `package.json` [#16403](https://github.com/pnpm/pnpm/issues/16403).
