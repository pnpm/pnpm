---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
---

Ensure `pnpm env remove` (and `rm`) cleans up dangling `node`, `npm`, and `npx` symlinks in the global bin and home directories when removing a Node.js version.
