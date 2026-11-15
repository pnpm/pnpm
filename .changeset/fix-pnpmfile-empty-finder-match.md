---
"pacquet": patch
---

`finders` in `.pnpmfile.cjs` that return an empty string no longer match packages in `pnpm list --find-by` or `pnpm why --find-by`.
