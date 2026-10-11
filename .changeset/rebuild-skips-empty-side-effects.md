---
"pacquet": patch
"@pnpm/napi": patch
---

A `rebuild` with `skipIfHasSideEffectsCache` no longer runs again the build of a package that the side-effects cache records as having changed nothing inside the package, such as a native addon whose optional compilation step failed. pnpm v11's `rebuild` skipped these builds too. An install still runs them, because their effect may lie outside the package.
