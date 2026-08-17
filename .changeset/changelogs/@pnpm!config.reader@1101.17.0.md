## 1101.17.0

### Minor Changes

- `--config.config-dir` no longer reaches the config through a project's `pnpm-workspace.yaml`, and neither do the `--config.` spellings of the other settings a project manifest may no longer contribute (`--config.pnpm-home-dir`, `--config.workspace-dir`, `--config.global-pkg-dir`, `--config.root-project-manifest-dir`). None of them was ever a supported way to set those directories: pnpm resolves them from the environment, and these flags took effect only because the project-manifest merge re-applied the command line afterwards. The dedicated flags, such as `--dir` and `--global-dir`, are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

- A project's `pnpm-workspace.yaml` can no longer choose where pnpm keeps its credentials, its own installation, or the registry it downloads its next version from. One of those settings is `configDir`, which decided where `pnpm login` writes the granted token. `bin`, `dir`, `globalBinDir`, `globalDir`, `npmrcAuthFile`, `pnpmHomeDir`, `stateDir`, `userconfig` and `workspaceDir` are ignored there now too, and pnpm warns about the ones it finds. `cacheDir` and `storeDir` are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).

### Patch Changes

- A setting written in kebab-case in the global `config.yaml` is now reported instead of being silently ignored [#13650](https://github.com/pnpm/pnpm/issues/13650).

- `packageExtensions` is now validated when the configuration is read, so a malformed entry (for instance a dependency range set to `null`) fails with an actionable error instead of crashing later during peer dependency resolution [#13756](https://github.com/pnpm/pnpm/issues/13756).

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.5
  - @pnpm/error@1100.1.2
  - @pnpm/hooks.pnpmfile@1100.0.27
  - @pnpm/pkg-manifest.utils@1100.4.0
  - @pnpm/workspace.project-manifest-reader@1100.0.24
  - @pnpm/workspace.workspace-manifest-reader@1100.1.6
