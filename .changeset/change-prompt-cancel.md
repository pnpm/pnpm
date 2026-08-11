---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Canceling a `pnpm change` prompt with Ctrl-c no longer prints a stack trace. It reports `Change canceled` and exits with a success status, like the other interactive commands [#13814](https://github.com/pnpm/pnpm/issues/13814).
