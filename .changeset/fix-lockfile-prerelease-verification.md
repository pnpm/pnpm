---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

Fixed lockfile verification to correctly handle pre-release versions. A pre-release version like `1.0.0-alpha` is now recognized as satisfying the `*` range in the lockfile, preventing unnecessary re-resolution.
