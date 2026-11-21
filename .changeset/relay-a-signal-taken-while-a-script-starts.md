---
"pacquet": patch
---

Fixed `pnpm run` hanging after an interrupt that arrived while a script was starting. The script was never asked to stop, so pnpm waited for it for as long as it ran. pnpm no longer ends on such an interrupt without the script it had just started.
