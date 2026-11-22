## 1101.4.13

### Patch Changes

- `pnpm install` now installs a workspace project located in a parent directory of the workspace root, such as one matched by `packages: ['..']` [#10096](https://github.com/pnpm/pnpm/issues/10096).

- `pnpm install` now runs the workspace root's `pnpm:devPreinstall` script when `sharedWorkspaceLockfile` is `false` [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).

- `pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

  - `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- `pnpm add` run from a workspace package directory that has no `package.json` yet now suggests creating one with `pnpm init` [#11345](https://github.com/pnpm/pnpm/issues/11345).

- `pnpm import` respects the symlink policy for the destination lockfile and skips Yarn patches outside the workspace.

- Updated `js-yaml` to v5. `pnpm-lock.yaml` is formatted the same as before.

- Fixed `pnpm link` resolving a quoted `~/` path from the current directory.

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- pnpm can read lockfiles with large YAML merge mappings or ordered maps without the excessive parsing time seen in some cases [#18246](https://github.com/pnpm/pnpm/issues/18246).

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` matching every package dependency when combining a GitHub Action selector with a negated package selector such as `!lodash`.

- `pnpm help import` now lists the output options, such as `--reporter`, that the command accepts [#7829](https://github.com/pnpm/pnpm/issues/7829).

- Approving the `minimumReleaseAge` prompt no longer reprints the list of packages it just showed. pnpm now confirms with one line that says how many entries it added to `minimumReleaseAgeExclude` [#15377](https://github.com/pnpm/pnpm/issues/15377).

- Fixed `pnpm unlink` in CI and in workspace subprojects. It now removes matching linked dependencies from the selected manifests.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.18
  - @pnpm/building.policy@1100.1.12
  - @pnpm/catalogs.config@1100.0.11
  - @pnpm/cli.common-cli-options-help@1100.0.4
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/config.pick-registry-for-package@1101.0.7
  - @pnpm/config.reader@1102.4.0
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/config.writer@1100.0.39
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/deps.github-actions@1100.1.21
  - @pnpm/deps.inspection.outdated@1100.1.44
  - @pnpm/deps.path@1101.0.11
  - @pnpm/deps.security.signatures@1102.0.14
  - @pnpm/deps.status@1100.1.37
  - @pnpm/error@1100.2.3
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/fs.read-modules-dir@1100.0.5
  - @pnpm/global.commands@1102.0.15
  - @pnpm/global.packages@1101.1.13
  - @pnpm/hooks.pnpmfile@1100.0.41
  - @pnpm/installing.context@1101.0.18
  - @pnpm/installing.dedupe.check@1100.1.21
  - @pnpm/installing.deps-installer@1104.2.12
  - @pnpm/installing.deps-resolver@1102.3.0
  - @pnpm/installing.env-installer@1103.0.18
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/network.auth-header@1101.1.20
  - @pnpm/network.fetch@1100.1.27
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/releasing.versioning@1100.3.11
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/store.connection-manager@1101.3.11
  - @pnpm/store.controller@1102.2.12
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.injected-deps-syncer@1100.0.51
  - @pnpm/workspace.project-manifest-reader@1100.1.8
  - @pnpm/workspace.project-manifest-writer@1100.0.24
  - @pnpm/workspace.projects-filter@1100.0.56
  - @pnpm/workspace.projects-graph@1100.0.50
  - @pnpm/workspace.projects-reader@1101.1.9
  - @pnpm/workspace.projects-sorter@1101.0.6
  - @pnpm/workspace.root-finder@1100.1.8
  - @pnpm/workspace.state@1100.0.57
  - @pnpm/workspace.task-scheduler@1100.0.9
  - @pnpm/workspace.workspace-manifest-reader@1100.3.4
  - @pnpm/workspace.workspace-manifest-writer@1100.2.13
