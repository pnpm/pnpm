---
"pacquet": patch
---

`pnpm pipeline` no longer fails when run in a project outside a Git work tree or on a system without `git`. Tasks in those projects run without caching, and pnpm prints a warning explaining why [#15601](https://github.com/pnpm/pnpm/issues/15601).
