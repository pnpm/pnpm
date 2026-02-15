---
"@pnpm/lockfile.preferred-versions": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.resolver-base": patch
pnpm: patch
---

Fixed a resolution bug that could cause `pnpm dedupe --check` to fail unexpectedly.

When adding new dependencies to `package.json`, pnpm generally reuses existing versions in the `pnpm-lock.yaml` if they are satisfied by the version range specifier. There was an edge case where pnpm would instead resolve to a newly released version of a dependency. This is particularly problematic for `pnpm dedupe --check`, since a new version of a dependency published to the NPM registry could cause this check to suddenly fail. For details of this bug, see [#10626](https://github.com/pnpm/pnpm/issues/10626). This bug has been fixed.

The fix necessitated a behavioral change: In some cases, pnpm was previously able to automatically dedupe a newly used dependency deep in the dependency graph without needing to run `pnpm dedupe`. This behavior was supported by the non-determinism that is now corrected. We believe fixing this non-determinism is more important than preserving an automatic dedupe heuristic that didn't handle all cases. The `pnpm dedupe` command can still be used to clean up dependencies that aren't automatically deduped on `pnpm install`.
