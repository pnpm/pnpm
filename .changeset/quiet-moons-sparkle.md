---
"@pnpm/real-hoist": patch
"@pnpm/headless": patch
"pnpm": patch
---

Peer dependencies of subdependencies should be installed, when `node-linker` is set to `hoisted` [#6680](https://github.com/pnpm/pnpm/pull/6680).
