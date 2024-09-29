---
"@pnpm/plugin-commands-store": patch
"pnpm": patch
---

Prevent ENOENT errors caused by running `store prune` in parallel.
