---
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm run` no longer reinstalls dependencies when a `node_modules` installed outside CI is used with `CI=true`, or the other way around. An install that never configured `enableGlobalVirtualStore` records no value for it, while `CI=true` resolves the same setting to `false`, and the two were compared as different values.
