---
"@pnpm/exec.commands": minor
"pnpm": minor
---

Added support for hidden scripts. Scripts starting with `.` are hidden and cannot be run directly via `pnpm run`. They can only be called from other scripts. Hidden scripts are also omitted from the `pnpm run` listing.
