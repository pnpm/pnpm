---
"pacquet": patch
---

`pnpm pipeline --dry-run` no longer writes a run report when no projects are selected. With `--json` it prints `{"tasks": []}` [#126](https://github.com/pnpm/tasks/issues/126).
