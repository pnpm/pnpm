---
"@pnpm/config": minor
"pnpm": minor
---

Global installs (`pnpm install -g`) and `pnpm dlx` now use the global virtual store by default. Packages are stored at `{storeDir}/links` instead of per-project `.pnpm` directories. This can be disabled by setting `enableGlobalVirtualStore: false`.
