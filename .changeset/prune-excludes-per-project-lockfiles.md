---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now work in workspaces with `sharedWorkspaceLockfile: false`. Once a recursive command has installed every project, pnpm drops an entry only if no project's lockfile records it. Undecided `allowBuilds` entries are pruned the same way [#14612](https://github.com/pnpm/pnpm/issues/14612).
