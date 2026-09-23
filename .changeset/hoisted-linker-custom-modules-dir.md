---
"pacquet": patch
---

With `nodeLinker: hoisted`, pnpm now installs the root project's dependencies into a custom `modulesDir` instead of `node_modules`. With a custom `modulesDir`, the virtual store and its `lock.yaml` now default to `<modulesDir>/.pnpm`.
