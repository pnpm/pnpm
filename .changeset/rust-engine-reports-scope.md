---
"pacquet": patch
---

`pnpm install`, `run`, `test`, `update`, `remove`, `link`, `unlink`, `prune`, and `rebuild` now print the workspace scope they resolved — `Scope: all 41 workspace projects`, or `Scope: 5 of 41 workspace projects` under a `--filter`. This is the confirmation that a filter selected what was intended [#13315](https://github.com/pnpm/pnpm/issues/13315).
