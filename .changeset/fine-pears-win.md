---
"@pnpm/deps.status": minor
"pnpm": patch
---

Fix a false negative in `verify-deps-before-run` when `node-linker` is `hoisted` and there is a workspace package without dependencies and `node_modules` directory [#9424](https://github.com/pnpm/pnpm/issues/9424).
