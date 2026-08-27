---
"pacquet": patch
---

Pacquet now strips exactly one leading path component from `./`-prefixed tarball entries, matching pnpm and npm's tar extraction semantics and keeping shared store keys consistent.
