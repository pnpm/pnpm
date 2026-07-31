## 1101.0.0

### Major Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

### Patch Changes

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.6
  - @pnpm/resolving.resolver-base@1101.0.0
  - @pnpm/types@1101.8.0
