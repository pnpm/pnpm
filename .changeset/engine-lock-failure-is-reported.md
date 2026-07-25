---
"pacquet": patch
---

When the pinned `packageManager` engine install cannot take its lock because the store cannot be written to, pnpm now reports that instead of quietly installing without the lock. A lock another process holds is unchanged — it is still waited for.
