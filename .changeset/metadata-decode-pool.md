---
"pacquet": patch
---

Lowered peak memory when pnpm downloads many packuments, such as during lockfile verification with `trustPolicy` on Linux. In one 1,800-entry workspace, a lockfile update peaked at 1.8 to 2.0 GB instead of 2.6 to 3.7 GB [#17020](https://github.com/pnpm/pnpm/issues/17020).
