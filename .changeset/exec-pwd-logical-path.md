---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm exec` now sets the `PWD` environment variable to the directory the command runs in. Shells and tools that read `PWD` now report the logical path of a workspace package reached through a symlink [#1550](https://github.com/pnpm/pnpm/issues/1550).
