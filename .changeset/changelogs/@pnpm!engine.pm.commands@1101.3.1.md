## 1101.3.1

### Patch Changes

- `pnpm update` keeps the explicit `=` operator of an exact version pin: a dependency saved as `=3.5.1` now updates to `=3.5.2` instead of the bare `3.5.2`. See pnpm/pnpm#13168.

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.24
  - @pnpm/building.policy@1100.0.17
  - @pnpm/cli.meta@1100.0.13
  - @pnpm/cli.utils@1101.0.21
  - @pnpm/config.pick-registry-for-package@1100.0.14
  - @pnpm/config.reader@1101.15.1
  - @pnpm/config.version-policy@1100.1.11
  - @pnpm/deps.graph-hasher@1100.2.14
  - @pnpm/deps.security.signatures@1101.2.9
  - @pnpm/global.commands@1100.0.42
  - @pnpm/global.packages@1100.0.15
  - @pnpm/installing.client@1100.3.1
  - @pnpm/installing.deps-restorer@1102.2.1
  - @pnpm/installing.env-installer@1102.0.14
  - @pnpm/lockfile.fs@1100.1.16
  - @pnpm/lockfile.types@1100.0.18
  - @pnpm/network.auth-header@1101.1.8
  - @pnpm/pkg-manifest.utils@1100.3.0
  - @pnpm/resolving.npm-resolver@1103.0.0
  - @pnpm/store.connection-manager@1100.3.14
  - @pnpm/store.controller@1102.0.10
  - @pnpm/types@1101.8.0
  - @pnpm/workspace.project-manifest-reader@1100.0.22
