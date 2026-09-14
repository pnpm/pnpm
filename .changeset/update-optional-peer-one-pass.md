---
"pacquet": patch
---

`pnpm update` now attaches an optional peer to a direct dependency in one pass when the peer version the lockfile named is no longer in the graph. A second `pnpm update` no longer changes the lockfile [#14895](https://github.com/pnpm/pnpm/issues/14895).
