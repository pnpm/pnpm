---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

`pnpm install` now fails the way a fresh resolution does when a package injected through a path, such as `file:../lib` or `../lib`, gives one of its dependencies a `catalog:` specifier. Before, the install could report the lockfile as up to date [#17722](https://github.com/pnpm/pnpm/pull/17722).
