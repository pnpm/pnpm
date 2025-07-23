---
"@pnpm/local-resolver": minor
---

Added `preserveAbsolutePaths` option to `resolveFromLocal`. When using `file:/path/to/package`, the absolute path will be preserved instead of being turned into a relative path.
