---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed cyclic peer dependencies sharing the wrong provider across workspace projects when the projects select different versions of that provider.
