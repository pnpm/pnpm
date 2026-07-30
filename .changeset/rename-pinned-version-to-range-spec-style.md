---
"@pnpm/types": minor
"@pnpm/resolving.npm-resolver": major
"@pnpm/resolving.resolver-base": major
"@pnpm/store.controller-types": major
"@pnpm/installing.deps-resolver": major
"@pnpm/installing.deps-installer": major
"@pnpm/installing.commands": patch
"@pnpm/global.commands": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
---

Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. No CLI behavior changes.
