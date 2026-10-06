---
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

With `enableGlobalVirtualStore`, an install that updates `node_modules` now restores the dependency links of a package in the global virtual store that an interrupted install left without them. Before, such an install kept the incomplete package if the project's `node_modules` already recorded it [#16642](https://github.com/pnpm/pnpm/issues/16642).
