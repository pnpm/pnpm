---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fixed `ERR_PNPM_MISSING_TIME` that occurred when `minimumReleaseAge` was active with abbreviated metadata caches. The resolver now falls through to fetch full metadata when the abbreviated cache lacks the `time` field, and gracefully handles the offline/preferOffline paths.
