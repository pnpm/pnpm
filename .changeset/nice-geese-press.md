---
"@pnpm/lockfile-utils": patch
---

When checking if the lockfile is up-to-date, an empty dependenciesMeta field in the manifest should be satisfied by a not set field in the lockfile.
