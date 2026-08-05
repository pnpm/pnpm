---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now apply the `beforePacking` pnpmfile hook to the manifest before a package is packed, matching the TypeScript CLI.
