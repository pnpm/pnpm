---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.package-requester": patch
"pnpm": patch
---

Delay fetching directory dependencies until direct dependencies are symlinked.
