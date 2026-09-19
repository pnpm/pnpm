---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --force` now removes obsolete dependency links inside virtual-store packages when their dependencies change [#15039](https://github.com/pnpm/pnpm/issues/15039).
