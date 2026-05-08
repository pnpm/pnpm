---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` should copy the `packageManager` from the workspace `package.json` to the deployed `package.json`.
