---
"pnpm": patch
---

Fix a bug causing pnpm to infinitely spawn itself when `manage-package-manager-versions=true` is set and the `.tools` directory is corrupt.
