---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` keeping stale peer bindings after dependency changes. Required dependencies no longer remain marked optional when another workspace project removes a shared package [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).
