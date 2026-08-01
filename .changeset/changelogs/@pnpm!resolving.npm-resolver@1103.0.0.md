## 1103.0.0

### Major Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

### Patch Changes

- The install summary no longer prints `(X is available)` when the registry's `dist-tags.latest` is still held back by the active `minimumReleaseAge` policy. The hint only ever names the actual latest tag, so an immature latest suppresses the hint instead of advertising the version pnpm just refused to install [#11698](https://github.com/pnpm/pnpm/issues/11698).

- `pnpm update` keeps the explicit `=` operator of an exact version pin: a dependency saved as `=3.5.1` now updates to `=3.5.2` instead of the bare `3.5.2`. See pnpm/pnpm#13168.

- Updated dependencies:
  - @pnpm/config.pick-registry-for-package@1100.0.14
  - @pnpm/config.version-policy@1100.1.11
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/pkg-manifest.utils@1100.3.0
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.14
  - @pnpm/resolving.registry.types@1100.1.8
  - @pnpm/resolving.resolver-base@1101.0.0
  - @pnpm/store.cafs@1100.1.17
  - @pnpm/types@1101.8.0
