---
"supi": patch
---

Don't create broken symlinks to skipped optional dependencies, when hoisting. This issue was already fixed in pnpm v5.13.7 for the case when the lockfile is up-to-date. This fixes the same issue for cases when the lockfile need updates. For instance, when adding a new package.
