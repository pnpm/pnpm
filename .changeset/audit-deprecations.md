---
"pnpm": minor
---

Added `pnpm audit deprecations`, which lists the installed packages that their registry marks as deprecated, with the projects or packages that bring each one in. It exits with code `1` if any package is deprecated. The deprecated line at the end of an install now points to this command.
