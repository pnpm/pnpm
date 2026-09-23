---
"pacquet": patch
---

`pnpm link` now warns when linking a package that declares one or more peer dependencies, explaining that the linked dependency will not resolve peer dependencies from the target `node_modules` and suggesting the `file:` protocol instead.
