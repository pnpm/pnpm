## 1100.3.0

### Minor Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

- The `save-prefix` setting now accepts `=`: newly added dependencies are saved with an explicit `=` operator (`=1.2.3`) instead of the setting being silently treated as the default `^`.

### Patch Changes

- `pnpm update` keeps the explicit `=` operator of an exact version pin: a dependency saved as `=3.5.1` now updates to `=3.5.2` instead of the bare `3.5.2`. See pnpm/pnpm#13168.

- Updated dependencies:
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/types@1101.8.0
