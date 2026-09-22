---
"pnpm": patch
"pacquet": patch
"@pnpm/exec.commands": patch
"@pnpm/exec.npm-lifecycle": patch
---

Fixed `pnpm exec` and `pnpm dlx` not setting `npm_execpath`, `INIT_CWD`, `npm_node_execpath`, and `NODE` for child processes. [#7037](https://github.com/pnpm/pnpm/issues/7037).
