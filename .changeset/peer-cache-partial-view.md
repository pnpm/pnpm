---
"pacquet": patch
---

Fixed a peer-resolution cache reuse that made cold-install lockfiles depend on traversal order in peer-cyclic dependency graphs: a verdict computed against a subtree truncated by the occurrence's own ancestors could be reused where the subtree is intact, silently dropping resolved peer variants from the lockfile [#13846](https://github.com/pnpm/pnpm/issues/13846), [#13865](https://github.com/pnpm/pnpm/issues/13865).
