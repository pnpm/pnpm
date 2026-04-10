---
"@pnpm/config.reader": minor
"@pnpm/store.connection-manager": minor
"@pnpm/deps.inspection.outdated": minor
"pnpm": minor
---

Added a new setting `minimumReleaseAgeStrict` that is `false` by default. When disabled (the default), pnpm falls back to versions that don't meet the `minimumReleaseAge` constraint if no mature versions satisfy the range being resolved. Set to `true` to fail installation instead.
