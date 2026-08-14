---
"pnpm": patch
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
---

Fixed a bug where `resolutionMode: lowest-direct` (or `lowest`) was ignored unless `minimumReleaseAge: 0` was explicitly set.
