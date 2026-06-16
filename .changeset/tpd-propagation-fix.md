---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed lockfile churn where a package's `transitivePeerDependencies` could be dropped (and shift between packages) when the package participates in a dependency cycle. A cycle re-entry resolves against truncated children, so it must not be cached as "pure"; otherwise sibling occurrences of the same package short-circuit and lose transitive peers depending on traversal order [#5108](https://github.com/pnpm/pnpm/issues/5108).
