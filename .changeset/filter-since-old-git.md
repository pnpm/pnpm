---
"@pnpm/workspace.projects-filter": patch
"pacquet": patch
"pnpm": patch
---

The `[<since>]` filter selector works again with Git 2.24 through 2.27 [#16561](https://github.com/pnpm/pnpm/issues/16561). With Git older than 2.24, the selector now fails with an error that names the required Git version.
