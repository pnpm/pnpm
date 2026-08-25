---
"pacquet": patch
---

Removing a package from a workspace now drops its importer entry from `pnpm-lock.yaml`, along with the dependencies only it needed. Previously the entry survived every later install, which kept those dependencies reachable and made the lockfile diverge from the one the TypeScript CLI writes [#13783](https://github.com/pnpm/pnpm/issues/13783).
