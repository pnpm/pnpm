---
"pacquet": patch
---

`pnpm exec` and `pnpm dlx` now set `npm_execpath`, `INIT_CWD`, `npm_node_execpath`, and `NODE` in child environments when Node.js is available. Stale inherited `NODE` and `npm_node_execpath` variables are cleared when Node.js cannot be found on PATH [#7037](https://github.com/pnpm/pnpm/issues/7037).
