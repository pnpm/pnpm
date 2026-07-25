---
"@pnpm/installing.deps-restorer": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/lockfile.filtering": minor
"pnpm": minor
---

A headless (frozen-lockfile) install now decides whether a package is optional from the dependency edges that reach it, the same way a fresh resolution does, instead of from the `optional` flag stored on the lockfile snapshot. A package that an installable package depends on through a regular `dependencies` edge is no longer treated as optional just because its whole subtree happens to hang off an `optionalDependencies` entry: an incompatible one now fails the install under `engineStrict` — and is installed with a warning without it — on both install paths. Packages reachable only through optional edges, or through a skipped parent, are still skipped [#13286](https://github.com/pnpm/pnpm/issues/13286).
