---
"pacquet": patch
---

`pnpm patch` and `pnpm patch-commit` now find a package installed from a `namedRegistries` alias by its version. For example, `pnpm patch foo@1.0.0` patches a dependency declared as `work:1.0.0`.
