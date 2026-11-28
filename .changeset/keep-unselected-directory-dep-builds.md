---
"pnpm": patch
---

`pnpm rebuild <pkg>` and `pnpm approve-builds <pkg>` now keep the build output of unselected `file:` directory dependencies under both isolated and hoisted node linkers.
