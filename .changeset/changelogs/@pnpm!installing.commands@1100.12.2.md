## 1100.12.2

### Patch Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

- The `save-prefix` setting now accepts `=`: newly added dependencies are saved with an explicit `=` operator (`=1.2.3`) instead of the setting being silently treated as the default `^`.

- Updated dependencies:
  - @pnpm/building.after-install@1102.0.14
  - @pnpm/building.policy@1100.0.17
  - @pnpm/cli.utils@1101.0.21
  - @pnpm/config.pick-registry-for-package@1100.0.14
  - @pnpm/config.reader@1101.15.1
  - @pnpm/config.version-policy@1100.1.11
  - @pnpm/config.writer@1100.0.20
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/deps.github-actions@1100.1.2
  - @pnpm/deps.inspection.outdated@1100.1.22
  - @pnpm/deps.path@1100.0.13
  - @pnpm/deps.security.signatures@1101.2.9
  - @pnpm/deps.status@1100.1.14
  - @pnpm/global.commands@1100.0.42
  - @pnpm/hooks.pnpmfile@1100.0.24
  - @pnpm/installing.context@1100.0.30
  - @pnpm/installing.dedupe.check@1100.1.7
  - @pnpm/installing.deps-installer@1103.0.0
  - @pnpm/installing.env-installer@1102.0.14
  - @pnpm/lockfile.fs@1100.1.16
  - @pnpm/lockfile.types@1100.0.18
  - @pnpm/network.auth-header@1101.1.8
  - @pnpm/network.fetch@1100.1.10
  - @pnpm/pkg-manifest.reader@1100.0.14
  - @pnpm/pkg-manifest.utils@1100.3.0
  - @pnpm/resolving.npm-resolver@1103.0.0
  - @pnpm/resolving.resolver-base@1101.0.0
  - @pnpm/store.connection-manager@1100.3.14
  - @pnpm/store.controller@1102.0.10
  - @pnpm/types@1101.8.0
  - @pnpm/workspace.project-manifest-reader@1100.0.22
  - @pnpm/workspace.project-manifest-writer@1100.0.13
  - @pnpm/workspace.projects-filter@1100.0.34
  - @pnpm/workspace.projects-graph@1100.0.30
  - @pnpm/workspace.projects-reader@1101.0.21
  - @pnpm/workspace.projects-sorter@1100.0.13
  - @pnpm/workspace.state@1100.0.35
  - @pnpm/workspace.workspace-manifest-reader@1100.1.4
  - @pnpm/workspace.workspace-manifest-writer@1100.0.20
