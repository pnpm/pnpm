---
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

Config dependencies now verify locked tarball locations against their registry before loading hooks. Legacy inline integrity pins cannot be replaced through the lockfile.
