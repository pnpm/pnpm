---
"pacquet": patch
---

`.modules.yaml` now records the dependencies of a skipped optional package in `skipped` as well, matching pnpm: when a platform-incompatible optional package is skipped, its own dependency subtree is not materialized either.
