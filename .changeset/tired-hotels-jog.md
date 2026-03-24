---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Fixed a bug preventing the `clearCache` function returned by `createNpmResolver` from properly clearing metadata cache.
