---
"@pnpm/config": major
"pnpm": major
---

Global installs (`pnpm install -g`) and `pnpm dlx` now use the global virtual store by default. Packages are stored at `{storeDir}/links` instead of per-project `.pnpm` directories. This can be disabled by setting `enableGlobalVirtualStore: false` [#10694](https://github.com/pnpm/pnpm/pull/10694).
