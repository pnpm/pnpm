---
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update` now preserves the existing range operator when updating a prerelease dependency. See pnpm/pnpm#7002.
