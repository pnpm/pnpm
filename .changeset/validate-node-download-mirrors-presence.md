---
"@pnpm/config.reader": patch
"pnpm": patch
---

Validate nodeDownloadMirrors presence check with Object.hasOwn on post-env-replace settings and reject explicit null values.
