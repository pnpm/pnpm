---
"pacquet": patch
---

`pnpm deprecate` and `pnpm unpublish` now work for packages whose registry metadata is larger than 10 MiB, such as `typescript`. They accept up to 50 MiB, the same limit as `pnpm owner`.
