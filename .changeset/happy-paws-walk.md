---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

When running "pnpm exec" from a subdirectory of a project, don't change the current working directory to the root of the project [#5759](https://github.com/pnpm/pnpm/issues/5759).
