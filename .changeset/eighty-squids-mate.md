---
"@pnpm/plugin-commands-installation": patch
---

Set `skipIfHasSideEffectsCache` to `true` when calling rebuild, fixing side effect caching issue when lockfile isn't shared [#6890](https://github.com/pnpm/pnpm/issues/6890).
