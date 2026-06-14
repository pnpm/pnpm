---
"@pnpm/lockfile.fs": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Close lockfile reads deterministically before rewriting lockfiles and keep pacquet's virtual store directory length aligned with pnpm on Windows.
