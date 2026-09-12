---
"pacquet": patch
---

`pnpm --filter "./packages/{app,lib}"` now selects either alternative. Quote the selector so the shell does not expand the braces first.
