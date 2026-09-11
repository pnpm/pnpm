---
"pacquet": patch
---

A dependency's own bins can no longer take over another package's bin shim. The POSIX shims pnpm generates looked up the shell helpers they need on `PATH`, which starts with `node_modules/.bin` while a script runs, so a dependency could supply one of those helpers and redirect the shim before it reached its target [#14837](https://github.com/pnpm/pnpm/issues/14837).
