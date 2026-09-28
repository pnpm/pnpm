---
"@pnpm/workspace.state": patch
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

With `nodeLinker: hoisted`, `pnpm install` now restores a workspace project's `node_modules` after it was deleted. Before, the install printed "Already up to date" and left the project without the dependencies nested under it.
