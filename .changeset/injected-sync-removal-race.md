---
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
---

Fixed injected workspace dependency synchronization failing with `EPERM` on Windows when removing nested directories.
