---
"pacquet": patch
---

When pnpm cancels a script, it now waits up to ten seconds for the system's process list. A process that a cancelled script started is no longer left running on a machine that answers more slowly than half a second.
