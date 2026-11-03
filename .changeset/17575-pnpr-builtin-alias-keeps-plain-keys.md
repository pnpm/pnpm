---
"@pnpm/pnpr": patch
---

A resolve request keeps plain lockfile keys for packages from its default registry, even when a prefix such as `npmjs` points at that registry [#17575](https://github.com/pnpm/pnpm/issues/17575).
