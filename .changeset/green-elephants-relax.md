---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fix recursive `pnpm update <name>@<version>` so an exact pinned update stays scoped to the requested version line: copies of the same package on another major line — or, for a `0.x` request, another minor line — keep their locked resolution instead of being re-resolved along with the target.
