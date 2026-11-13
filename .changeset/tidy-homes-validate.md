---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm setup` rejects a `PNPM_HOME` value that would split the saved `PATH` [pnpm/tasks#315](https://github.com/pnpm/tasks/issues/315).
