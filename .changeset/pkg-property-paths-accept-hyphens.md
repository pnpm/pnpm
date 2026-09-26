---
"@pnpm/object.property-path": patch
"pacquet": patch
"pnpm": patch
---

`pnpm pkg get` and `pnpm pkg set` now accept hyphens inside a dot-notation property path, so `pnpm pkg get dependencies.some-package-name` reads the key instead of failing with `ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH`. The bracketed and quoted forms already worked and are unchanged.
