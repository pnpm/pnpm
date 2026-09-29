---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` no longer copies the workspace root's `devEngines` into the deployed `package.json`. npm enforces `devEngines.packageManager`, so `npm run` failed in a deploy directory with `EBADDEVENGINES`. The deployed `package.json` still gets the root's `packageManager` field. If the root pins an exact pnpm version in `devEngines.packageManager`, that version is written as the `packageManager` field [#16403](https://github.com/pnpm/pnpm/issues/16403).
