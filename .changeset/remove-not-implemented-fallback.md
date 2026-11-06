---
"pacquet": patch
---

Removed the internal not-implemented command fallback. Every npm command is now implemented, so unrecognized commands fall through to the external-subcommand handling again.
