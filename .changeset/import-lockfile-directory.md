---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm import` now leaves the project-local lockfile unchanged when `lockfileDir` points to another directory. Failed imports restore the destination lockfile. Imports that use a branch lockfile leave the shared lockfile unchanged [#14563](https://github.com/pnpm/pnpm/issues/14563).
