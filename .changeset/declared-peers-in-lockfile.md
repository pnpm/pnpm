---
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

A package that lists the same dependency in `dependencies` and as an optional peer now gets the peer bound when a project adds a provider for it later. The lockfile now records every peer a package declares, including one its own dependency supplies [#17056](https://github.com/pnpm/pnpm/issues/17056).
