---
"@pnpm/plugin-commands-env": patch
"@pnpm/link-bins": patch
"pnpm": patch
---

New directories should be prepended to NODE_PATH in command shims, not appended.
