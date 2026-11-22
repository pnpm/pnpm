---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fixed `pnpm unlink` in CI and in workspace subprojects. It now removes matching linked dependencies from the selected manifests.
