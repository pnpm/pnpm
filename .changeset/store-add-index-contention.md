---
"pacquet": patch
---

`pnpm store add` now fails if it cannot write a package to the store index, for example when another process holds the index locked. It also indexes tarball URLs that have no integrity value [pnpm/pnpm#18194](https://github.com/pnpm/pnpm/issues/18194).
