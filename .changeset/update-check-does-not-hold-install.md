---
"pacquet": patch
---

`pnpm install` and `pnpm add` no longer wait for the daily update check when the registry does not answer it. The check now has one second after the command finishes to report a newer pnpm. Otherwise the command finishes without the notice, and the check is not repeated until the next day. On a registry that accepted the connection but never answered, an install that had nothing to do used to take minutes [#16839](https://github.com/pnpm/pnpm/issues/16839).
