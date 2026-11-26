---
"pnpm": patch
---

`pnpm install` now drops a removed optional peer from the lockfile when you change the version of the dependency that declares it [#18482](https://github.com/pnpm/pnpm/issues/18482).
