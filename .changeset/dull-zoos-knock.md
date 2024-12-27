---
"@pnpm/exportable-manifest": patch
"pnpm": patch
---

Fixed `publish`/`pack` error with workspace dependencies with relative paths [#8904](https://github.com/pnpm/pnpm/pull/8904). It was broken in `v9.4.0` ([398472c](https://github.com/pnpm/pnpm/commit/398472c)).
