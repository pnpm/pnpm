---
"@pnpm/types": minor
"@pnpm/config.reader": minor
"@pnpm/workspace.task-scheduler": minor
"@pnpm/exec.commands": minor
"pnpm": minor
"pacquet": minor
---

Added per-task concurrency limits to workspace task orchestration. Set `tasks.<name>.concurrency` in `pnpm-workspace.yaml` to limit how many instances of that task may run across workspace projects at once:

```yaml
tasks:
  build:
    concurrency: 2
```
