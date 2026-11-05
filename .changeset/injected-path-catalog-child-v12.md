---
"pacquet": patch
---

When a package injected through a path, such as `file:../lib` or `../lib`, gives one of its dependencies a `catalog:` specifier, `pnpm install --frozen-lockfile` now reports the lockfile as outdated. For a bare path such as `../lib`, `pnpm install` now also fails the way a fresh resolution does. Before, these installs accepted the existing lockfile [#17722](https://github.com/pnpm/pnpm/pull/17722).
