---
"pacquet": patch
---

Fixed filtered and recursive `pnpm run` and `pnpm exec` hanging when a script reads from the terminal. Interactive prompts work again in a script that pnpm never runs alongside another one, such as a single `--filter`ed project, `--workspace-concurrency=1`, a dependency chain, or a task declaring `concurrency: 1` [#14397](https://github.com/pnpm/pnpm/issues/14397).
