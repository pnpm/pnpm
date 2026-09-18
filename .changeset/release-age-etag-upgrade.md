---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed `minimumReleaseAge` being skipped for packages served by a registry that returns the same ETag for abbreviated and full package metadata [pnpm/pnpm#14925](https://github.com/pnpm/pnpm/issues/14925).
