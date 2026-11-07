---
"pacquet": patch
---

`pnpm install` now keeps waiting when another install holds a global virtual store slot lock. It stops with an error if the lock cannot be acquired [pnpm/tasks#284](https://github.com/pnpm/tasks/issues/284).
