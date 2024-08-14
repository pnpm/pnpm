---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

When dlx uses cache, use the real directory path not the symlink to the cache.
