---
"pacquet": patch
---

**Breaking change from pnpm v11.** Under `engineStrict`, an install fails when an incompatible package is reached through a regular `dependencies` edge of an installable package, even when that whole subtree hangs off an `optionalDependencies` entry. pnpm v11 installs the package and emits an install-check warning instead. Packages reachable only through optional edges, or through a package that was itself skipped, are still skipped in both versions [#13286](https://github.com/pnpm/pnpm/issues/13286).
