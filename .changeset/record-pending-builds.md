---
"pacquet": minor
---

`pnpm install --ignore-scripts` now records the builds it skipped in `node_modules/.modules.yaml`'s `pendingBuilds`, so `pnpm rebuild --pending` runs them afterwards instead of finding nothing to do. Both the dependencies whose build scripts were suppressed and the workspace projects whose own install scripts were suppressed are recorded, and an install that removes a package drops it from the list.
