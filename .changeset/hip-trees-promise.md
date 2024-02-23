---
"@pnpm/resolve-dependencies": minor
"@pnpm/merge-lockfile-changes": minor
"@pnpm/lockfile-types": minor
"@pnpm/prune-lockfile": minor
"@pnpm/lockfile-file": minor
"@pnpm/core": minor
"@pnpm/pnpmfile": minor
"pnpm": minor
---

The checksum of the `.pnpmfile.cjs` is saved into the lockfile. If the pnpmfile gets modified, the lockfile is reanalyzed to apply the changes [#7662](https://github.com/pnpm/pnpm/pull/7662).
