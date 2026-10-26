---
"pacquet": patch
---

Fixed `pnpm run` waiting forever after an interrupt when a script left behind a process that ignores it. pnpm now gives the script's processes thirty seconds to stop, then ends what is still running as it exits.
