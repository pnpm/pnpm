---
"@pnpm/lockfile.merger": patch
"pnpm": patch
---

Fixed a crash in the lockfile merger when merging non-semver version strings (e.g. `link:`, `file:`, git URLs).
