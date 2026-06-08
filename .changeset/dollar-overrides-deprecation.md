---
"@pnpm/config.reader": patch
"pnpm": patch
---

Using the `$` version reference syntax in `overrides` (e.g. `"react": "$react"`) now prints a deprecation warning. The syntax still works, but [catalogs](https://pnpm.io/catalogs) are the recommended way to keep an overridden version in sync with the rest of the workspace. Reference a catalog entry with the `catalog:` protocol instead.
