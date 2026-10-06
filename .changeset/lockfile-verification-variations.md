---
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Lockfile verification now checks the tarballs inside a `variations` resolution against the registry. A `name@version` lockfile entry with an empty `variations` resolution is now rejected.
