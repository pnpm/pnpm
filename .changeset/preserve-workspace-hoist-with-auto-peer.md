---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.hoist": patch
"pnpm": patch
---

`pnpm dedupe` now preserves a hoisted workspace package when another package uses a separately installed peer with the same name [pnpm/pnpm#16935](https://github.com/pnpm/pnpm/issues/16935).
