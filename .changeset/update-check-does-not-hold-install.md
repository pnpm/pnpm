---
"pacquet": patch
---

`pnpm install` and `pnpm add` no longer wait for the daily update check. The check runs alongside the command, as it does in pnpm v11; when the command finishes first, it finishes without the notice and the check is made again next time. On a registry that accepted the connection but never answered, an install that had nothing to do used to take minutes [#16839](https://github.com/pnpm/pnpm/issues/16839).
