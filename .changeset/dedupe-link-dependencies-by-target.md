---
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.modules-cleaner": patch
"pacquet": patch
"pnpm": patch
---

With `dedupeDirectDeps`, `pnpm install` now compares `link:` dependencies by the directory they point at. A project's `link:` dependency is removed when the workspace root links the same directory under a different path. It is kept when the root's `link:` dependency has the same text but points at another directory.

With `nodeLinker: hoisted` and `dedupeDirectDeps`, `pnpm install` now removes a project's `link:` dependency that the workspace root provides.

With `nodeLinker: hoisted`, `pnpm install` now reports each `link:` dependency it removes.
