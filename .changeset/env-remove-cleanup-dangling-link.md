---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
"pacquet": patch
---

Ensure `pnpm env remove` (and `rm`) cleans up dangling `node`, `npm`, and `npx` symlinks and shims in the global bin and home directories when removing a Node.js version.
