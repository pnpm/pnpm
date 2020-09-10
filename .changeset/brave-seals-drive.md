---
"@pnpm/package-store": patch
---

When `package-import-method` is set to `auto`, cloning is only tried once. If it fails, it is not retried for other packages.
