---
"pacquet": patch
---

`pnpm exec` and `pnpm dlx` now set `npm_execpath` and `INIT_CWD` for child processes, set `npm_node_execpath` and `NODE` when Node.js is resolvable from the parent environment, and clear stale inherited Node.js environment variables when it is not [#7037](https://github.com/pnpm/pnpm/issues/7037).
