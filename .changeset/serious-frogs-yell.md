---
"@pnpm/resolve-dependencies": major
---

Removed the `neverBuiltDependencies` option. In order to ignore scripts of some dependencies, use the new `allowBuild`. `allowBuild` is a function that accepts the package name and returns `true` if the package should be allowed to build.
