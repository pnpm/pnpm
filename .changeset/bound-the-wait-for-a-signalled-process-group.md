---
"pacquet": patch
---

Fixed `pnpm run` waiting forever after an interrupt when a script left behind a process that ignores it. pnpm now waits thirty seconds for the script's processes to stop and then exits, leaving any that are still running.
