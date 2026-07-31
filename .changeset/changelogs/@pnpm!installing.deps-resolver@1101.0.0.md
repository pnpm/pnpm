## 1101.0.0

### Major Changes

- Renamed the `PinnedVersion` type to `RangeSpecStyle` and the `pinnedVersion` option fields to `rangeSpecStyle`: the value selects the operator a specifier is saved with, not a pin. `whichVersionIsPinned` is now `inferRangeSpecStyle`, and the new `rangeSpecGranularity` helper collapses the `exact` spelling to its `patch` range width. `@pnpm/types` keeps `PinnedVersion` as a deprecated alias of `RangeSpecStyle`. The rename itself changes no CLI behavior; the `=`-pin handling is described in its own changesets.

### Patch Changes

- Fixed empty `bundledDependencies` and `bundleDependencies` arrays causing nondeterministic lockfile changes. See pnpm/pnpm#13123.

- Preserve a workspace dependency's `link:` entry when a run does not target it — e.g. `pnpm update <other-pkg>` (with or without `--recursive`), or a plain install after a root/catalog dependency change — with `injectWorkspacePackages`, instead of spuriously rewriting it to a peer-suffixed `file:` protocol. See pnpm/pnpm#10433.

- Updated dependencies:
  - @pnpm/config.version-policy@1100.1.11
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/deps.graph-hasher@1100.2.14
  - @pnpm/deps.path@1100.0.13
  - @pnpm/fetching.pick-fetcher@1100.1.5
  - @pnpm/fs.symlink-dependency@1100.0.16
  - @pnpm/hooks.types@1100.2.5
  - @pnpm/lockfile.preferred-versions@1100.0.27
  - @pnpm/lockfile.pruner@1100.0.18
  - @pnpm/lockfile.types@1100.0.18
  - @pnpm/lockfile.utils@1100.1.7
  - @pnpm/patching.config@1100.0.14
  - @pnpm/pkg-manifest.reader@1100.0.14
  - @pnpm/pkg-manifest.utils@1100.3.0
  - @pnpm/resolving.npm-resolver@1103.0.0
  - @pnpm/resolving.resolver-base@1101.0.0
  - @pnpm/store.controller-types@1101.0.0
  - @pnpm/types@1101.8.0
