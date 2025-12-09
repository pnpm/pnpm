---
"@pnpm/resolve-dependencies": minor
"@pnpm/core": minor
"@pnpm/types": minor
"@pnpm/config": minor
---

Added a new setting `registrySubdepsOnly` that disallows non-registry dependencies (git, tarball URLs) in subdependencies. When enabled, direct dependencies can still use any source, but transitive dependencies must come from a package registry.
