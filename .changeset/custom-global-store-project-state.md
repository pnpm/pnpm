---
"pacquet": patch
---

Keep each project's current lockfile and hidden hoisted dependencies local when `virtualStoreDir` selects a shared global virtual store. Installs in other projects no longer overwrite that state [pnpm/tasks#47](https://github.com/pnpm/tasks/issues/47).
