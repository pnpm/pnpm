---
"pacquet": patch
---

`pnpm --filter ./packages/{app,lib}` now selects either alternative. Brace alternatives nest, may span a path separator, and combine with the other wildcards.
