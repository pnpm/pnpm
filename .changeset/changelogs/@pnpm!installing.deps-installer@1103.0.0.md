## 1103.0.0

### Major Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.24
  - @pnpm/bins.remover@1100.0.18
  - @pnpm/building.after-install@1102.0.14
  - @pnpm/building.during-install@1102.0.13
  - @pnpm/building.policy@1100.0.17
  - @pnpm/config.normalize-registries@1100.0.13
  - @pnpm/config.version-policy@1100.1.11
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/deps.graph-hasher@1100.2.14
  - @pnpm/deps.path@1100.0.13
  - @pnpm/exec.lifecycle@1100.1.10
  - @pnpm/fs.symlink-dependency@1100.0.16
  - @pnpm/hooks.read-package-hook@1100.2.1
  - @pnpm/hooks.types@1100.2.5
  - @pnpm/installing.context@1100.0.30
  - @pnpm/installing.deps-resolver@1101.0.0
  - @pnpm/installing.deps-restorer@1102.2.1
  - @pnpm/installing.linking.direct-dep-linker@1100.0.16
  - @pnpm/installing.linking.hoist@1100.0.24
  - @pnpm/installing.linking.modules-cleaner@1100.1.17
  - @pnpm/installing.modules-yaml@1100.0.14
  - @pnpm/installing.package-requester@1102.1.8
  - @pnpm/lockfile.filtering@1100.2.1
  - @pnpm/lockfile.fs@1100.1.16
  - @pnpm/lockfile.preferred-versions@1100.0.27
  - @pnpm/lockfile.pruner@1100.0.18
  - @pnpm/lockfile.settings-checker@1100.1.9
  - @pnpm/lockfile.to-pnp@1100.1.11
  - @pnpm/lockfile.utils@1100.1.7
  - @pnpm/lockfile.verification@1100.0.30
  - @pnpm/lockfile.walker@1100.0.18
  - @pnpm/network.auth-header@1101.1.8
  - @pnpm/patching.config@1100.0.14
  - @pnpm/pkg-manifest.utils@1100.3.0
  - @pnpm/pnpr.client@1.3.9
  - @pnpm/resolving.resolver-base@1101.0.0
  - @pnpm/store.controller-types@1101.0.0
  - @pnpm/types@1101.8.0
  - @pnpm/workspace.project-manifest-reader@1100.0.22
