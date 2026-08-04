---
"pacquet": patch
---

A stray non-directory entry in `node_modules` no longer fails an install. Files placed next to the installed dependencies are skipped rather than reported as an unreadable manifest.
