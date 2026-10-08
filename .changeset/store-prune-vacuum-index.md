---
"pacquet": patch
---

`pnpm store prune` now compacts the store's `index.db` after removing package entries, so the file shrinks again [#16717](https://github.com/pnpm/pnpm/issues/16717).
