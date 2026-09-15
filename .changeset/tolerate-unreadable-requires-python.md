---
"pacquet": patch
"@pnpm/pnpr": patch
---

Python resolution no longer fails on a release whose `Requires-Python` is not a version specifier, such as the trailing comma in `openpyxl` 3.0.x. pnpm now reads such a value as if the release declared no interpreter range [#14910](https://github.com/pnpm/pnpm/issues/14910).
