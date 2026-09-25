---
"@pnpm/patching.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm patch-commit` now falls back to copying package files when hard linking fails.
