---
"@pnpm/reviewing.dependencies-hierarchy": minor
---

The `path` field for direct dependencies returned from `buildDependenciesHierarchy` was incorrect if the dependency used the `workspace:` or `link:` protocols.
