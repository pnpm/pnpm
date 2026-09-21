---
"pacquet": patch
---

`pnpm store prune` now removes `pnpm dlx` cache directories that have outlived `dlxCacheMaxAge`, along with the prepare directories a newer dlx run superseded [#15171](https://github.com/pnpm/pnpm/issues/15171).
