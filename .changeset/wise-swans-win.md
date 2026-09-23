---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

A lockfile entry whose resolution is unchanged now keeps its recorded `deprecated` message. An outdated metadata cache no longer replaces it with an older message when someone else regenerates the lockfile [#5772](https://github.com/pnpm/pnpm/issues/5772).
