---
"@pnpm/config.reader": minor
"@pnpm/store.connection-manager": minor
"@pnpm/deps.inspection.outdated": minor
"pnpm": minor
---

Added a new setting `minimumReleaseAgeLoose` that is `true` by default. When enabled, pnpm falls back to versions that don't meet the `minimumReleaseAge` constraint if no mature versions satisfy the range being resolved.
