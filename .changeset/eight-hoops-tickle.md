---
"pacquet": patch
---

Fixed installation of `eslint` and `@eslint-community/eslint-utils`: the compatibility database no longer injects a dependency on `estree`, which is not an npm package (its types live in `@types/estree`), so every fresh resolve of those packages failed with a 404.
