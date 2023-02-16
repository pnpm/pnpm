---
"@pnpm/core": patch
"pnpm": patch
---

Don't retry installation if the integrity checksum of a package failed and no lockfile was present.
