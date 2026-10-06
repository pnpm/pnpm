---
"@pnpm/exec.prepare-package": patch
"pnpm": patch
---

Fixed git dependency `#path:` subpath extraction traversing intermediate symlinks outside the repository root.
