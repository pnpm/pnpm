---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now ship the whole gitignored directory that `main`, `browser`, or `bin` points into, as npm does. Previously only the `main` and `bin` files themselves were packed, and `browser` was not packed at all [#16908](https://github.com/pnpm/pnpm/issues/16908).
