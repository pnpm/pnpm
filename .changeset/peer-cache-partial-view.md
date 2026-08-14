---
"pacquet": patch
---

Fixed non-deterministic lockfiles on cold installs of projects with cyclic peer dependencies: resolved peer variants could silently drop from the lockfile depending on traversal order [#13846](https://github.com/pnpm/pnpm/issues/13846), [#13865](https://github.com/pnpm/pnpm/issues/13865).
