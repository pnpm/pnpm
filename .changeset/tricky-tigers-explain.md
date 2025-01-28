---
"@pnpm/resolve-dependencies": patch
---

When a dependency exists in both dependencies and peerDependencies and the corresponding peerDependenciesMeta option is true, the dependency should be installed.
