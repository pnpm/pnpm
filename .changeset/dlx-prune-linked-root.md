---
"@pnpm/store.commands": patch
"pnpm": patch
---

`pnpm store prune` now leaves a `dlx` cache root that is a symlink or Windows junction untouched. Cleanup no longer removes directories through that link.
