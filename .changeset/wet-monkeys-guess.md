---
"@pnpm/headless": patch
---

Don't create broken symlinks in subprojects that have external symlinks, when the linked dependencies are excluded from the lockfile.
