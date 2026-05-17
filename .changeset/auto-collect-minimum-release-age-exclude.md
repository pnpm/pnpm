---
"@pnpm/store.controller-types": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Tightened the loose-mode (`minimumReleaseAgeStrict: false`) story so the bypass becomes explicit on disk instead of silent:

1. Fresh resolutions in loose mode that fall back to a version newer than the `minimumReleaseAge` cutoff (resolver's lowest-version fallback) auto-collect the picked `name@version` into the workspace manifest's `minimumReleaseAgeExclude`. A single info message lists the additions; entries already on the list are left alone.
2. The post-resolution lockfile verifier introduced in #11583 now runs in loose mode too — every accepted-immature pin must be on `minimumReleaseAgeExclude`, just like strict mode requires. A lockfile produced under a weaker (or absent) policy that still has immature entries is rejected the same way strict mode would reject it. With the auto-collect from point 1 keeping the manifest in sync, the steady-state install runs cleanly: `pnpm add foo@immature` populates the exclude list on the way in, and subsequent installs (including `--frozen-lockfile` in CI) verify against the populated list.
