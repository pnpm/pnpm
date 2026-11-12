---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

Fixed scripts and their descendants surviving when pnpm is killed during script startup.
