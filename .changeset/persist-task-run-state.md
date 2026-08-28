---
"@pnpm/workspace.task-scheduler": minor
"@pnpm/exec.commands": minor
"pnpm": minor
"pacquet": minor
---

Persist completed recursive tasks so `--resume-from` skips exactly the work that passed during a matching interrupted or failed `pnpm -r run` / `pnpm -r exec` invocation. When no compatible state exists, pnpm retains its graph-based resume behavior.
