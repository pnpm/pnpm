---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

On Windows, the `.cmd` command shims in `node_modules/.bin` now keep a `%` in the project path. Before, cmd.exe expanded it as a variable reference, so the command received a mangled `NODE_PATH` [#15716](https://github.com/pnpm/pnpm/issues/15716).
