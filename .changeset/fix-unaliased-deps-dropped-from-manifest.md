---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed a regression introduced by [#11711](https://github.com/pnpm/pnpm/pull/11711) where `pnpm add <github-shorthand>` (and any other wanted-dependency whose alias can't be parsed from the user-supplied spec, e.g. tarball URLs or `pnpm/test-git-fetch#sha`) was silently dropped from the manifest update and from `pendingBuilds`. The alias-keyed lookup added in that PR couldn't find a `wantedDependency` whose `alias` was `undefined` at parse time but resolved to a package name only after fetching, so the entry never made it into `specsToUpsert`. Restored the original index-based pairing between `directDependencies` and `wantedDependencies`; the catalog-protocol preservation that PR was originally fixing is unaffected because it's driven by `rdd.catalogLookup.userSpecifiedBareSpecifier`, not by the lookup. Fixes the three `rebuilds dependencies` / `rebuilds specific dependencies` / `rebuild with pending option` failures in `building/commands/test/build/index.ts`.
