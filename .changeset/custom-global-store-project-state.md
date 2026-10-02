---
"pacquet": patch
---

pnpm now keeps each project's current lockfile and hidden hoisted dependencies in its own `node_modules/.pnpm` when `virtualStoreDir` points at a shared global virtual store. `--virtual-store-dir` now sets the global virtual store's location too [pnpm/tasks#47](https://github.com/pnpm/tasks/issues/47).
