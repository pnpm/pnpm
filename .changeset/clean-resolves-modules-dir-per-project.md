---
"pacquet": patch
---

`pnpm clean` / `pnpm purge` run from a workspace subdirectory now remove each project's own `node_modules` instead of emptying the workspace root's for every project [#14239](https://github.com/pnpm/pnpm/issues/14239). A custom `modulesDir` is resolved against each project directory too.
