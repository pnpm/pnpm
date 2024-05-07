---
"@pnpm/npm-resolver": patch
"@pnpm/default-reporter": patch
"pnpm": patch
---

Print a better error message when `resolution-mode` is set to `time-based` and the registry fails to return the `"time"` field in the package's metadata.
