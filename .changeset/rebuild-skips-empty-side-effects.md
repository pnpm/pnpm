---
"pacquet": patch
"@pnpm/napi": patch
---

A `rebuild` with `skipIfHasSideEffectsCache` ran the build scripts of a package again when the side-effects cache recorded that its build changed nothing inside the package, such as a native addon whose optional compilation failed. Such a rebuild now skips these packages, as pnpm v11's `rebuild` did. An install still runs their build scripts.
