---
"pacquet": patch
---

`pnpm --filter "./packages/{app,lib}"` now selects either alternative. Brace alternatives nest, may span a path separator, and combine with the other wildcards. Quote the selector so the shell does not expand the braces first.
