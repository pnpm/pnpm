---
"pacquet": patch
---

pnpm now runs the install scripts of a git-hosted dependency each time it links the dependency from the store into a new location. This includes a new global virtual store slot and a fresh `node_modules`. Before, only the first install that fetched the dependency built it, and later ones linked it unbuilt. This also fixes dependencies that are already in the store [#17224](https://github.com/pnpm/pnpm/issues/17224).
