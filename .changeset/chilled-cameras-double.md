---
"@pnpm/constants": major
"pnpm": minor
---

Metadata directory version bumped to force fresh cache after we shipped a fix to the metadata write function. This change is backward compatible as install doesn't require a metadata cache.
