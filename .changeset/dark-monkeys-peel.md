---
"@pnpm/plugin-commands-store": patch
"pnpm": patch
---

Fix `pnpm store path` and `pnpm store status` using workspace root for path resolution when `storeDir` is relative [#10290](https://github.com/pnpm/pnpm/issues/10290).
