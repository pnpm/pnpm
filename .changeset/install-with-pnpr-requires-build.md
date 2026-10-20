---
"pacquet": minor
---

`pnpm install` now reads `dist.pnpr.requiresBuild` from pnpr package metadata and `requiresBuild` from pnpr resolution frames to plan build work before tarballs finish downloading.
