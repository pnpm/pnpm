---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Fix race condition of symlink creations caused by multiple parallel `dlx` processes.
