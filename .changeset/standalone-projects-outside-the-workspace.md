---
"@pnpm/workspace.package-patterns": patch
"@pnpm/workspace.projects-reader": patch
"@pnpm/workspace.root-finder": patch
"pacquet": patch
"pnpm": patch
---

A command run in a directory that the workspace does not include now acts on that project alone. A directory is not part of the workspace when no pattern in the `packages` setting selects it, or when a `!` pattern excludes it. `pnpm install` there used to install every project in the workspace [#3561](https://github.com/pnpm/pnpm/issues/3561).
