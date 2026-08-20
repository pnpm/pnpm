---
"pacquet": patch
---

`nodeLinker: hoisted` no longer installs a second copy of a package when two workspace projects resolve one of its peers differently. Both projects share the copy in the root `node_modules`, the way pnpm 10 and 11 place it.
