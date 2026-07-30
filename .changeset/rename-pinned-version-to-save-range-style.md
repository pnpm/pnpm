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

Renamed the `PinnedVersion` type to `SaveRangeStyle` and the `pinnedVersion` option fields to `saveRangeStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferSaveRangeStyle`, and the new `saveRangeGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `SaveRangeStyle`. No CLI behavior changes.
