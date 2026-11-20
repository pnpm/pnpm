---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

An auto-installed optional peer is now resolved to a version its declared peer range accepts, even when the workspace root depends on that package at a version outside the range. Previously the root's version was used and then reported as an unmet optional peer [#13867](https://github.com/pnpm/pnpm/issues/13867).
