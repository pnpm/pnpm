---
"@pnpm/plugin-commands-store": patch
---

Avoid the "too many open files error" on `pnpm store status` command.
Limit the concurrency of verifying dependency contents.
