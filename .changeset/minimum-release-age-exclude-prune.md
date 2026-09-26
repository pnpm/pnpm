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

Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.
