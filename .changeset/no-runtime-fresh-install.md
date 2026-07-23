---
"pacquet": patch
---

`pnpm install --no-runtime` now works without `--frozen-lockfile`: on a fresh install, runtime dependencies are resolved and recorded in the lockfile, but their archives are not downloaded and their bins are not linked.
