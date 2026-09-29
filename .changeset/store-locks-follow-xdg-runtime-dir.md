---
"pacquet": patch
---

pnpm now creates its store operation locks and other per-user lock files in `$XDG_RUNTIME_DIR` when it points to a directory the user owns. Otherwise, pnpm still uses `/tmp`. Sandboxes that block writes to `/tmp` can point `XDG_RUNTIME_DIR` at a writable directory [#16390](https://github.com/pnpm/pnpm/issues/16390).
