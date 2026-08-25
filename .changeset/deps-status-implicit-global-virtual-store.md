---
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm run` no longer reinstalls dependencies when a `node_modules` directory installed outside CI is used with `CI=true`, or the other way around.
