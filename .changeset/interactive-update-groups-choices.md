---
"pacquet": patch
---

`pnpm update --interactive` now groups the dependencies it offers by dependency type — `dependencies`, `devDependencies`, `optionalDependencies`, `peerDependencies`, and GitHub Actions each get their own heading — and lays each group out as a column-aligned table with a `Package`/`Current`/`Target`/`URL` header, instead of one flat list.
