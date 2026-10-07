---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
"pacquet": patch
---

Removal overrides such as `"debug>supports-color": "-"` now also apply to an optional peer that a package declares only in `peerDependenciesMeta` [#16681](https://github.com/pnpm/pnpm/issues/16681).
