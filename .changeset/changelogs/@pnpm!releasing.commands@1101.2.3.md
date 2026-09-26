## 1101.2.3

### Patch Changes

- `pnpm change check` now validates the pending change intents in `.changeset/`. It fails when an intent names a package that is not in the workspace or cannot be released.

- `pnpm deploy` now copies the `packageManager` and `devEngines.packageManager` fields of the workspace root `package.json` into the deployed `package.json`, unless the deployed project pins a package manager itself [#9079](https://github.com/pnpm/pnpm/issues/9079).

- `pnpm deploy` no longer triggers an install when running scripts in a read-only deployed filesystem [#11617](https://github.com/pnpm/pnpm/issues/11617).

- `pnpm deploy` now respects `--package-import-method` passed on the command line and reports the package import method correctly [pnpm/pnpm#7593](https://github.com/pnpm/pnpm/issues/7593).

- `pnpm deploy` now puts the virtual store at `virtualStoreDir`, resolved against the deploy directory. A shared-lockfile deploy records `virtualStoreDir` in the deployed `pnpm-workspace.yaml`. With the global virtual store enabled or an absolute `virtualStoreDir`, the deploy still uses `node_modules/.pnpm` [#8787](https://github.com/pnpm/pnpm/issues/8787).

- `pnpm publish` and `pnpm pack` now report an error when a bin script has a shebang line ending with CRLF [pnpm/pnpm#7311](https://github.com/pnpm/pnpm/issues/7311).

- The `publish` command now resolves `workspace:` dependencies from workspace manifests when `node_modules` is not installed. Previously, publishing without `node_modules` failed with `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL`. pnpm/pnpm#6567

- pnpm no longer crashes on startup when the temporary directory set by `TMPDIR`, `TEMP`, or `TMP` does not exist [#4960](https://github.com/pnpm/pnpm/issues/4960).

- `pnpm deploy --legacy` no longer rewrites `node_modules/.pnpm-workspace-state-v1.json` in the source workspace. The next `verifyDepsBeforeRun` check there reported the workspace as out of date [#15352](https://github.com/pnpm/pnpm/issues/15352).

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm pack` now includes exactly one `package.json` in the archive when the project uses an alternative manifest format. The manifest is included even when `.npmignore` or `files` excludes the source file.

- `pnpm pack` now preserves file executable permissions in the packed tarball when source files are executable on disk.

- `pnpm pack`, `pnpm deploy`, and installs of local directory dependencies now keep symlinks that point to files or directories included in the package. `pnpm pack` leaves out symlinks that point outside the package [#8208](https://github.com/pnpm/pnpm/issues/8208).

- `pnpm version` now applies pending bumps to private workspace packages. A private package's changelog is written to its committed `CHANGELOG.md`, also when `versioning.changelog.storage` is `registry`
  [pnpm/pnpm#13736](https://github.com/pnpm/pnpm/issues/13736),
  [pnpm/pnpm#13519](https://github.com/pnpm/pnpm/issues/13519).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- `pnpm publish` now honors `publishConfig["@scope:registry"]` for a package in that scope. It takes precedence over the registry set for the same scope in `.npmrc` and over `publishConfig.registry` [#12071](https://github.com/pnpm/pnpm/issues/12071).

- `pnpm pack` and `pnpm publish` now include bundled dependencies when using the isolated linker. This covers workspace packages and the dependencies of each bundled package. Bundled dependencies are also included when `publishConfig.directory` selects a build directory [pnpm/pnpm#1643](https://github.com/pnpm/pnpm/issues/1643).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/deps.path@1101.0.4
  - @pnpm/engine.runtime.commands@1101.1.4
  - @pnpm/engine.runtime.node-resolver@1101.3.3
  - @pnpm/error@1100.2.0
  - @pnpm/exec.lifecycle@1100.1.19
  - @pnpm/exec.pnpm-cli-runner@1100.0.4
  - @pnpm/fetching.directory-fetcher@1100.0.35
  - @pnpm/fs.indexed-pkg-importer@1100.0.30
  - @pnpm/fs.packlist@1100.0.5
  - @pnpm/installing.client@1100.3.11
  - @pnpm/installing.commands@1101.4.0
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/network.git-utils@1100.0.5
  - @pnpm/network.web-auth@1101.6.1
  - @pnpm/releasing.exportable-manifest@1100.3.3
  - @pnpm/releasing.versioning@1100.3.3
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/resolving.registry.types@1100.2.1
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
  - @pnpm/workspace.projects-filter@1100.0.44
  - @pnpm/workspace.projects-graph@1100.0.39
  - @pnpm/workspace.projects-sorter@1101.0.1
  - @pnpm/workspace.task-scheduler@1100.0.2
  - @pnpm/workspace.workspace-manifest-writer@1100.2.2
