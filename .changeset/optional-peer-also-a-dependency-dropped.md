---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

pnpm no longer drops a dependency that a package also declares as an optional peer dependency, for example `ajv` of `ajv-formats`. Since 11.28.0, in some dependency graphs the package got the dependency neither as a dependency nor as a peer, and failed to import it [#16901](https://github.com/pnpm/pnpm/issues/16901).
