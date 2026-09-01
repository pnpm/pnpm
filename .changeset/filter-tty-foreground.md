---
"pacquet": patch
---

Fixed filtered and recursive `pnpm run` and `pnpm exec` hanging when a script reads from the terminal. A script the workspace never runs alongside another one — a single `--filter`ed project, `--workspace-concurrency=1`, a dependency chain, or a task declaring `concurrency: 1` — now stays in the terminal's foreground process group, so interactive prompts work again [#14397](https://github.com/pnpm/pnpm/issues/14397).
