---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now reports the lockfile as outdated when an injected workspace package changes a dependency's named-registry range (such as `work:^2.0.0`) to one the locked version no longer satisfies. Before, it installed the old version.
