---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fix `minimumReleaseAgeExclude` handling in npm resolution fast paths so excluded packages do not get pinned to stale versions. Excludes are honored consistently during `publishedBy` metadata selection and cache-mtime shortcuts.
