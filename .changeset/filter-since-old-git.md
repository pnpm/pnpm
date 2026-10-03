---
"@pnpm/workspace.projects-filter": patch
"pacquet": patch
"pnpm": patch
---

The `[<since>]` filter selector works again with git versions older than 2.28. pnpm passed `git diff` an option those versions do not know, so they failed with a usage error [#16561](https://github.com/pnpm/pnpm/issues/16561).
