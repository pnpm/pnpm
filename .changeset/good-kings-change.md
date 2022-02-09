---
"@pnpm/package-store": patch
"pnpm": patch
---

This fixes an issue introduced in pnpm v6.30.0.

When a package is not linked to `node_modules`, no info message should be printed about it being "relinked" from the store [#4314](https://github.com/pnpm/pnpm/issues/4314).
