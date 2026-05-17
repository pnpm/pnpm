---
"@pnpm/store.controller-types": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

When `minimumReleaseAgeStrict` is `false`, any package versions installed despite being newer than the `minimumReleaseAge` cutoff are now auto-collected into the workspace manifest's `minimumReleaseAgeExclude` list in `pnpm-workspace.yaml`. This covers both the resolver's lowest-version fallback when no mature version satisfies the requested range, and the `peekManifestFromStore` fast path that reuses lockfile-pinned versions without re-running the maturity check. A single info message lists the additions so the user sees exactly what was persisted; entries already present are left alone. With the new behavior, a subsequent install — including one promoted to strict mode — accepts the same versions without prompting the user to declare each exclusion by hand, making the loose-mode bypass explicit on disk instead of silent.
