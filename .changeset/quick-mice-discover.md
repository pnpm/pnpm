---
"pacquet": patch
---

Speed up workspace discovery for literal directories and conventional trailing-star patterns.

Workspace patterns now follow the same dot-directory rule as pnpm 11: a wildcard no longer matches a dot-prefixed directory, so `packages/*` and `**` skip `packages/.cache` and `.git`. A pattern that names a dot-prefixed directory still matches it, as `packages/.cache` and `packages/.*` do.
