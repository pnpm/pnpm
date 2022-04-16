---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

When `pnpm exec` is running a command in a workspace project, the commands that are in the dependencies of that workspace project should be in the PATH [#4481](https://github.com/pnpm/pnpm/issues/4481).
