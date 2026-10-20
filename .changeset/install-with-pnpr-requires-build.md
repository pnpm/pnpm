---
"pacquet": minor
---

`pnpm install` now reads `dist.pnpr.requiresBuild` from pnpr package metadata to plan build work before tarballs finish downloading. It also reads `requiresBuild` from pnpr resolution frames.
