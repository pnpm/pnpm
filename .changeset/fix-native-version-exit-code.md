---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Fixed `pnpm version` to correctly return the exit code instead of terminating the process prematurely. This ensures that the command works correctly with the pnpm reporter and in recursive mode.
