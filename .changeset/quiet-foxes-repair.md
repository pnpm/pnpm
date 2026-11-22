---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now fails when the `injected` setting of a dependency of a local directory dependency no longer matches the lockfile [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).
