---
"@pnpm/headless": patch
"@pnpm/core": patch
"pnpm": patch
---

When `hoisted-workspace-packages` is `true` don't hoist the root package even if it has a name. Otherwise we would create a circular symlink.
