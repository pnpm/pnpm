---
"pacquet": minor
---

Added concurrency groups for tasks. A task in `pnpm-workspace.yaml` can name a `concurrencyGroup`. The new `concurrencyGroups` setting gives each group a limit. At most that many tasks of the group run at once on the machine, counted across every `pnpm run`, script shortcut such as `pnpm test`, and `pnpm pipeline` invocation, including the ones running in other worktrees. A task past the limit waits for a running one to finish and prints which processes hold the slots. A script that calls `pnpm run` for a task of the same group runs under the slot its parent holds.

```yaml
tasks:
  test:rust:
    concurrencyGroup: cargo
concurrencyGroups:
  cargo: 2
```
