---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` no longer copies the workspace root's `devEngines` into the deployed `package.json`. If the root pins an exact pnpm version in `devEngines.packageManager`, the deployed `package.json` gets it as its `packageManager` field [#16403](https://github.com/pnpm/pnpm/issues/16403).
