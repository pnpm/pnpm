---
"@pnpm/plugin-commands-store": patch
"pnpm": patch
---

Prevent `ENOENT` errors caused by running `store prune` in parallel [#8586](https://github.com/pnpm/pnpm/pull/8586).
