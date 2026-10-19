---
"pacquet": patch
---

`pnpm install`, `pnpm add`, and `pnpm update` now reuse the lockfile's resolution for dependencies that take part in a dependency cycle, such as `mlly` and `pkg-types`. They re-resolved those packages and everything that depends on them from the registry before [pnpm/tasks#130](https://github.com/pnpm/tasks/issues/130).
