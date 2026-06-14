---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

Close lockfile reads deterministically before rewriting lockfiles.
