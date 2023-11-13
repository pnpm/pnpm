---
"@pnpm/real-hoist": patch
"@pnpm/lockfile-utils": patch
"pnpm": patch
---

Fixed out-of-memory exception that was happening on dependencies with many peer dependencies, when `node-linker` was set to `hoisted` [#6227](https://github.com/pnpm/pnpm/issues/6227).
