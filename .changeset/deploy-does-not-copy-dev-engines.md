---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` no longer copies the workspace root's `devEngines` into the deployed `package.json`. npm enforces `devEngines.packageManager` by default — a deploy output whose `devEngines` named pnpm made `npm run <script>` fail with `EBADDEVENGINES` — so the deployed manifest now inherits only the `packageManager` pin, which is the field corepack reads. When the root declares no `packageManager` but an exact `devEngines.packageManager` version, that version is written as the `packageManager` pin instead, so the pin the workspace uses is still preserved [#16403](https://github.com/pnpm/pnpm/issues/16403).
