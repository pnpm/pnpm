---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Improve the `pnpm add` workspace-root warning when the command is run from a workspace-matched directory that does not yet have a package manifest. In that case, pnpm now suggests initializing the package first, for example with `pnpm init`, while keeping the existing root warning for directories outside the workspace package patterns.
