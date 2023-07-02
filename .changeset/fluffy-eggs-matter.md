---
"@pnpm/resolve-dependencies": patch
---

Replacing object spread with a prototype chain, avoiding extra memory allocations in resolveDependenciesOfImporters.
