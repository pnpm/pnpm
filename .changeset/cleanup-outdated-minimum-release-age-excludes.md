---
"@pnpm/config.reader": minor
"@pnpm/config.version-policy": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/lockfile.utils": minor
"@pnpm/workspace.workspace-manifest-writer": minor
"pacquet": minor
"pnpm": minor
---

Added a new setting `cleanupOutdatedMinimumReleaseAgeExcludes`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune outdated entries from `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`: versions that are no longer resolved in the lockfile are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer present in the lockfile are removed as well.
