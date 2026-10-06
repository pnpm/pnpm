---
"pacquet": patch
---

With `enableGlobalVirtualStore`, `pnpm install` now repairs a package in the global virtual store that an interrupted install left without some of its dependency links or package files. Before, a project whose `node_modules` already recorded that package skipped it, and the package failed at runtime with `Cannot find module` until `pnpm install --force` [#16642](https://github.com/pnpm/pnpm/issues/16642).
