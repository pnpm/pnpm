---
"pacquet": patch
---

Global packages installed with `pnpm add -g` are now linked from the global virtual store, so global packages that share dependencies share one copy. Set `virtualStoreType: project` to give each global install its own virtual store.
