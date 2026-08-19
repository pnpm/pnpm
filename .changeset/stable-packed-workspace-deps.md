---
"@pnpm/releasing.exportable-manifest": patch
"pnpm": patch
---

Packed workspace package manifests now preserve dependency order, making repeated `pnpm pack` output deterministic [#10167](https://github.com/pnpm/pnpm/issues/10167).
