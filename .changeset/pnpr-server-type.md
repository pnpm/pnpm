---
"@pnpm/types": patch
"@pnpm/config.reader": patch
"@pnpm/resolving.tarball-url": patch
"pnpm": patch
---

Added the `pnpr` value to the `serverType` registry setting. pnpm leaves the tarball URLs of a pnpr registry out of `pnpm-lock.yaml`.
