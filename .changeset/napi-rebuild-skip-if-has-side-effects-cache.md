---
"@pnpm/napi": minor
---

`rebuild` accepts `skipIfHasSideEffectsCache`. With it set, a package whose build the side-effects cache already holds is restored from the cache, and its build scripts do not run again.
