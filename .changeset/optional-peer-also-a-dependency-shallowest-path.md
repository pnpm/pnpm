---
"pacquet": patch
---

pnpm now decides whether a package's dependency that it also declares as an optional peer is linked as a dependency or as a peer from the shallowest path that reaches the package. The lockfile entry of such a package can change on the next install [#16901](https://github.com/pnpm/pnpm/issues/16901).
