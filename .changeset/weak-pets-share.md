---
"@pnpm/hoist": patch
"pnpm": patch
---

Hoisting with symlinks should not override external symlinks and directories in the root of node_modules.
