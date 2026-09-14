## 1102.2.0

### Minor Changes

- `nodeDownloadMirrors` can now be set in the global config file (`config.yaml`) and through the `PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS` environment variable, so a Node.js download mirror can be configured once for a machine instead of in every workspace [#12124](https://github.com/pnpm/pnpm/issues/12124), [#13611](https://github.com/pnpm/pnpm/issues/13611).

  ```sh
  PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS='{"release":"https://npmmirror.com/mirrors/node/"}'
  ```

- Added a new setting `trustPolicyExcludePrune` (default: `false`). When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `trustPolicyExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

### Patch Changes

- pnpm now reads the `packageManager`, `devEngines.packageManager` and runtime pins from the workspace root's `package.json` when `lockfileDir` is set. A project that moved its lockfile lost the pins it declared there [#14633](https://github.com/pnpm/pnpm/issues/14633).

- A `registry` or `@scope:registry` set in an `.npmrc` now wins over the registry a `pnpm login` credential stored in the global `config.yaml` points at. Previously, after logging in to one registry, installs in a project whose `.npmrc` named a private registry went to the logged-in registry instead. They now go to the registry the `.npmrc` names [#14614](https://github.com/pnpm/pnpm/issues/14614).

- Updated dependencies:
  - @pnpm/hooks.pnpmfile@1100.0.31
  - @pnpm/pkg-manifest.utils@1100.4.4
  - @pnpm/workspace.project-manifest-reader@1100.0.28
