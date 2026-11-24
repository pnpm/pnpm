---
"pnpm": minor
---

Added `pnpm audit engines`, which lists the installed packages that don't support the current Node.js version or platform, with what each one wants and the projects or packages that bring it in. It exits with code `1` if any package is incompatible. The lines about unsupported Node.js versions and platforms at the end of an install now point to this command.
