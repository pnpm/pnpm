---
"@pnpm/installing.env-installer": patch
"pnpm": patch
---

pnpm now applies `trustPolicy` to the pnpm version a project pins in `packageManager` or `devEngines.packageManager`. A pinned version whose trust evidence was downgraded fails the command before pnpm switches to it or records it in the lockfile [#16319](https://github.com/pnpm/pnpm/issues/16319).
