---
"pacquet": patch
---

Keep non-concurrent filtered `run` and recursive `exec` children in the foreground process group, so interactive scripts can read from the terminal instead of being stopped by `SIGTTIN` [#14397](https://github.com/pnpm/pnpm/issues/14397).
