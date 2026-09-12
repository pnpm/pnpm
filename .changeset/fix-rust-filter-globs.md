---
"pacquet": patch
---

`pnpm --filter` directory selectors now support `?` wildcards and character classes such as `[ab]`. A `*` or `?` wildcard no longer selects a directory whose name starts with a dot, matching pnpm 11.
