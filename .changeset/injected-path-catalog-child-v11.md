---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

When a package injected through a path, such as `file:../lib` or `../lib`, gives one of its dependencies a `catalog:` specifier, `pnpm install` now fails the way a fresh resolution does, and `pnpm install --frozen-lockfile` reports the lockfile as outdated. Before, both could accept the existing lockfile [#17722](https://github.com/pnpm/pnpm/pull/17722).
