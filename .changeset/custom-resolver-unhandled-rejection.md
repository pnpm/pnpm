---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

A custom resolver's `shouldRefreshResolution` hook that rejects no longer crashes pnpm with an unhandled rejection when another hook has already asked for a refresh.
