---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed an issue where dependency resolution against registries without `ETag` or `Last-Modified` support (such as Azure DevOps Artifacts) re-downloaded package metadata unnecessarily on range specifications [#13976](https://github.com/pnpm/pnpm/issues/13976).
