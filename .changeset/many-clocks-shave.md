---
"@pnpm/real-hoist": patch
"pnpm": patch
---

Fixed out-of-memory exception that was happening on dependencies with many peer dependencies, when `node-linker` was set to `hoisted`.
