---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Prevent `EBUSY` errors caused by calling `symlinkDir` in parallel `dlx` processes.
