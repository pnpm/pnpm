---
"pacquet": patch
---

`pnpm outdated` now checks dependencies when the project has no lockfile, for example with `lockfile: false`. The `Current` column shows the installed version, or the lowest version allowed by the declared range if the dependency is not installed [pnpm/pnpm#17811](https://github.com/pnpm/pnpm/issues/17811).
