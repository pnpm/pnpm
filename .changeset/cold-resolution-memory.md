---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Reduced peak memory usage during cold-cache dependency resolution. The metadata fetch is memoized for the whole resolution phase, and it was retaining each package's raw registry response body (used only to mirror the response to disk) for that entire time. The raw body is now released as soon as the disk mirror is written. On large graphs that fetch full metadata (e.g. with `minimumReleaseAge` or `trustPolicy` enabled) this cuts peak RSS by roughly 30%, back in line with pnpm 10. The resolved lockfile is unchanged.
