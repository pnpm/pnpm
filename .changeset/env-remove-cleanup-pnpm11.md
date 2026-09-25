---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
---

Clean up dangling Node executables and symlinks during `pnpm env remove`. Surviving global commands remain intact.
