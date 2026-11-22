---
"pacquet": patch
---

`pnpm install --force` now removes obsolete dependency links inside virtual-store packages when their dependencies change. Invalid dependency names are ignored during obsolete-link cleanup [#15039](https://github.com/pnpm/pnpm/issues/15039).
