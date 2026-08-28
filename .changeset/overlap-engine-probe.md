---
"pacquet": patch
---

Rebuilding `node_modules` from an up-to-date lockfile is up to ~200 ms faster: the `node --version` probe that installability checks and store keying need now runs concurrently with the store's warm-cache reads instead of before them.
