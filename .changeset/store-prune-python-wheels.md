---
"pacquet": patch
---

`pnpm store prune` no longer removes files of the wheels that an installed Python project uses, so `pnpm install --offline` can recreate its environment. Pruning now also removes the store index entries of Python and Cargo archives whose files it deleted.
