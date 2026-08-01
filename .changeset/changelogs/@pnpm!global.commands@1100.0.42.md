## 1100.0.42

### Patch Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

- The `save-prefix` setting now accepts `=`: newly added dependencies are saved with an explicit `=` operator (`=1.2.3`) instead of the setting being silently treated as the default `^`.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.24
  - @pnpm/bins.remover@1100.0.18
  - @pnpm/bins.resolver@1100.0.13
  - @pnpm/cli.utils@1101.0.21
  - @pnpm/config.reader@1101.15.1
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/deps.inspection.list@1100.0.31
  - @pnpm/global.packages@1100.0.15
  - @pnpm/installing.deps-installer@1103.0.0
  - @pnpm/pkg-manifest.reader@1100.0.14
  - @pnpm/pkg-manifest.utils@1100.3.0
  - @pnpm/store.connection-manager@1100.3.14
  - @pnpm/types@1101.8.0
