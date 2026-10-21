## 11.28.7

### Patch Changes

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when an injected workspace package depends on a local directory under another name, such as `"renamed": "file:../b"` [#17130](https://github.com/pnpm/pnpm/issues/17130).

- `pnpm install` now succeeds when `node_modules` is a symlink to an existing directory. Dependency hoisting previously rejected a symlinked `node_modules` as an invalid destination [pnpm/pnpm#17157](https://github.com/pnpm/pnpm/issues/17157).

- When another workspace project adds a different version of a package, a project's auto-installed peer on that package now moves to the version a fresh install picks. It used to stay on its locked version while any other project still accepted that version [#17058](https://github.com/pnpm/pnpm/issues/17058).

- When the reader of pnpm's output exits early, as in `pnpm outdated | head`, pnpm now finishes the command and exits with the command's own exit code [pnpm/tasks#133](https://github.com/pnpm/tasks/issues/133). It used to exit with code 0 at the first write to the closed pipe. That hid the result of commands such as `pnpm outdated` and stopped `pnpm clean` before it removed anything.

- Concurrent installs that import the same package into one project now all complete. One of them used to fail with `ENOTEMPTY` when its copy of the package collided with the copy another install had already placed [pnpm/tasks#48](https://github.com/pnpm/tasks/issues/48).

- The warning about unrecognized settings in `pnpm-workspace.yaml` and the global `config.yaml` now names `addMissingPeerTypes`, `autoDedupe`, `bootConfig`, `macosBackup`, `nodePackageMap`, `permissions`, `pipelineBase`, `publishWaitTimeout`, `python`, `remoteCache`, `saveTypes`, `skills`, `storeIndexFormat`, and `tools` as pnpm v12 settings [pnpm/tasks#167](https://github.com/pnpm/tasks/issues/167).

- A package that lists the same dependency in `dependencies` and as an optional peer now gets the peer bound when a project adds a provider for it later. The lockfile now records every peer a package declares, including one its own dependency supplies [#17056](https://github.com/pnpm/pnpm/issues/17056).

- `pnpm deploy` now records the peer dependencies of the workspace packages it copies into the deployed lockfile. Before, pnpm rejected that lockfile with `--frozen-lockfile` when a deployed workspace package declared a peer and `injectWorkspacePackages` was off [#17093](https://github.com/pnpm/pnpm/issues/17093).

- Fixed `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` failing inside a directory created by `pnpm deploy`. `pnpm deploy` now writes self-contained workspace and lockfile settings. `pnpm deploy` also prunes unused `patchedDependencies` and preserves workspace-level dependency resolutions on deployed directory dependencies.

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- pnpm now records the pinned pnpm version in the `pnpm-lock.yaml` under the `lockfileDir` set in `pnpm-workspace.yaml`, and resolves a relative `lockfileDir` from the workspace root. pnpm used to write a second `pnpm-lock.yaml` at the workspace root.

- With `enableGlobalVirtualStore`, a Node.js resolve hook that a script registers now sees the importer as `context.parentURL` after pnpm's `NODE_PATH` fallback fails to find a bare specifier. Hooks that resolve tsconfig `paths` aliases from the importer failed with `ERR_MODULE_NOT_FOUND` [#16979](https://github.com/pnpm/pnpm/issues/16979).

- pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).

- Filtered installs remove obsolete dependencies from the installed lockfile while preserving dependencies still needed by unselected projects.

- Trusted publishing with OIDC now works with registries whose URL has a path, such as `https://registry.example/npm/private/`. pnpm previously sent the token exchange request to the root of the registry host.

- `pnpm owner ls` now lists the owners of a package on the npm registry. It reported every package as not found.

- `pnpm sbom` now reads project lockfiles in workspaces with `sharedWorkspaceLockfile: false`, including filtered and split output.

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of an injected workspace package no longer match the lockfile. Before, it installed the stale dependencies when `sharedWorkspaceLockfile` was `false` [#17008](https://github.com/pnpm/pnpm/issues/17008).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a local directory dependency, such as an injected workspace package, changes a dependency to a different tarball URL or git commit [#17129](https://github.com/pnpm/pnpm/issues/17129).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of a transitive injected or local directory dependency no longer match the lockfile, even when that dependency is not a workspace importer. Before, only the immediate directory dependency's children were checked, so a stale transitive snapshot was installed silently [#17128](https://github.com/pnpm/pnpm/issues/17128).

- With `minimumReleaseAge` set and full metadata in use, pnpm now keeps using cached metadata that the registry confirms is current, also when that metadata has no publish times. Previously, pnpm downloaded such metadata again on every install.

- `pnpm add` no longer warns that a gist dependency was replaced from a different source when the new specifier names the same gist in another form, such as `gist:<id>` and `https://gist.github.com/<user>/<id>`.

- With `sharedWorkspaceLockfile: false`, `pnpm install` now runs the `preResolution` hooks and custom resolvers and fetchers of the workspace root's pnpmfile for every project, next to the project's own. A global pnpmfile now runs once per project, and each project's lockfile records the checksum of the root pnpmfile when the project has none of its own. Before, a project's own pnpmfile replaced these root hooks, the global hooks ran twice, and edits to the root pnpmfile could go unnoticed.

- A filtered `pnpm install --no-lockfile` with `hoistWorkspacePackages` now hoists every workspace project into `node_modules/.pnpm/node_modules`. It used to hoist only the selected projects [pnpm/tasks#168](https://github.com/pnpm/tasks/issues/168).

- A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them. It no longer fails when the package links to one directory twice or links back to a directory above the link. `pnpm install` with `deployAllFiles` no longer fails on such a link back either.

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- An injected workspace dependency now becomes a link when its dependencies match the workspace package's own, also on an install that leaves the package's installed dependencies as they are. Before, such an install kept it as a copy [#17094](https://github.com/pnpm/pnpm/issues/17094).

- An injected workspace package that was linked now becomes a copy when the project that depends on it starts providing one of its peers at a version the package's own workspace context lacks. The link used to stay until that dependency was updated, so the package could not find the peer the project provides [#17057](https://github.com/pnpm/pnpm/issues/17057).

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- Fixed `pnpm install` failing on macOS with `EISDIR` when it creates store directories in a group-writable store under a umask that removes owner read, such as `0o477`.

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

- pnpm no longer writes the pinned pnpm version to `pnpm-lock.yaml` when it cannot read every setting and the running pnpm already matches the pin.

- An optional peer that pnpm adds to a project now keeps the version the lockfile recorded for that project only while the dependency graph still holds it. Otherwise it follows its provider to a version the graph holds. Before, pnpm kept a higher version that only the lockfile held, which left two copies of the peer [#16443](https://github.com/pnpm/pnpm/issues/16443).

- `pnpm install --prod` no longer drops a dependency that a package declares both as an optional peer and as its own dependency, when the package resolved it as its own dependency and a dev dependency provides the same package under another alias.

- `verifyDepsBeforeRun` and repeat `pnpm install` runs now apply `overrides` before comparing a modified `package.json` with `pnpm-lock.yaml` [#17127](https://github.com/pnpm/pnpm/issues/17127).

- pnpm now measures a `pnpm.overrides` entry written as a `workspace:` path, such as `workspace:./packages/foo`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote. The install then linked a directory that does not exist, or failed if workspace packages were injected [#17010](https://github.com/pnpm/pnpm/issues/17010).

- Fixed patching in workspaces with `sharedWorkspaceLockfile: false`. `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` now checks configured patches across all workspace projects before reporting unused patches.

- `pmOnFail` and `lockfileDir` in `pnpm-workspace.yaml` now stay in force when another setting in that file cannot be read. pnpm used to switch to the pinned version despite `pmOnFail: ignore` in that case.

- `pnpm pkg fix` now converts a `repository` string such as `github:pnpm/example` to the object form, as `npm pkg fix` does. It also normalizes a GitHub, GitLab, or Bitbucket repository URL the way `npm pkg fix` does [pnpm/pnpm#16980](https://github.com/pnpm/pnpm/issues/16980).

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now keep exclusions for resolved configuration dependencies and their transitive dependencies [pnpm/pnpm#17142](https://github.com/pnpm/pnpm/issues/17142).

- `verifyDepsBeforeRun` no longer treats the workspace root as up to date after a filtered install with `pruneDirectDependencies: true` skipped it [pnpm/tasks#181](https://github.com/pnpm/tasks/issues/181).

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` (`-r`) to prune selected workspace projects without removing dependencies of unselected projects.

- `pnpm install` now removes config dependencies that are no longer declared, including when the entire `configDependencies` setting is removed. Frozen installs reject a lockfile that still records removed config dependencies.

- `pnpm add` now reads the package name correctly from a tarball URL under a registry URL that has a path, such as `https://example.com/npm/`. Before, the registry path became part of the name, so a scoped package failed with `ERR_PNPM_INVALID_PACKAGE_NAME`.

- pnpm now fails with `ERR_PNPM_REGISTRY_NAME_MISMATCH` if a registry returns metadata for a package other than the one requested. Before, pnpm installed the package under the name in the metadata, so `allowBuilds` rules for the requested package did not apply to it.

- With `minimumReleaseAge` set, packages listed in `minimumReleaseAgeExclude` now resolve from cached metadata without a registry request when the lockfile already has the version their range picks. Packages resolve the same way without `minimumReleaseAge`.

- With `minimumReleaseAge` set, pnpm sends one conditional metadata request instead of two for a package whose cached metadata already holds the full document, but without every version's publish time.

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. The check that runs before a script now holds the workspace projects the script's project depends on. It agrees with the repeat-install check on which projects a filtered install left uninstalled, including with per-project lockfiles. Workspace dependencies declared with `link:` are now included in the workspace project graph, so `--filter=<project>...` selects them and, with `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, the check before a script still requires them to be installed. A filtered install no longer forgets the ignored build scripts of the projects it did not select.

- With `minimumReleaseAge` set, pnpm now revalidates a package's cached full metadata with its ETag, so an unchanged registry answers with `304`. Before, if the registry did not list a publish time for every version, pnpm downloaded that metadata again whenever the cached copy was older than `minimumReleaseAge`.

- pnpm now ignores a `PNPM_CONFIG_*` environment variable for a URL setting, such as `PNPM_CONFIG_REGISTRY`, when its value is not a URL. Such a value used to make every command fail with `Invalid URL`.

- `pnpm sbom` now reports the repository of a package whose `repository` field is a `gist:` shorthand.

- `pnpm self-update` now updates the project's pnpm pin when Homebrew installed pnpm or Corepack runs it. The global pnpm is left alone [#17001](https://github.com/pnpm/pnpm/issues/17001).

- `pnpm setup` now completes when Windows registry environment values cannot be read or written. It prints PowerShell commands to configure `PNPM_HOME` and `PATH` without registry access [pnpm/pnpm#17167](https://github.com/pnpm/pnpm/issues/17167).

- `pnpm setup` now succeeds when it cannot tell which shell configuration file to update, for example in a container where `SHELL` is not set. It installs pnpm and prints the `PNPM_HOME` and `PATH` lines to add yourself. Previously it failed with `ERR_PNPM_UNKNOWN_SHELL`, and so did the installer script at get.pnpm.io [#16973](https://github.com/pnpm/pnpm/issues/16973).

- `pnpm licenses list` reports no licenses for workspace projects not recorded in the lockfile. It errors when `pnpm-lock.yaml` is missing. Empty results now use the requested text or JSON format [pnpm/tasks#174](https://github.com/pnpm/tasks/issues/174).

- pnpm now applies `trustPolicy` to the pnpm version a project pins in `packageManager` or `devEngines.packageManager`. A pinned version whose trust evidence was downgraded fails the command before pnpm switches to it or records it in the lockfile [#16319](https://github.com/pnpm/pnpm/issues/16319).

- With `sharedWorkspaceLockfile: false`, a script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of the package it ran in. Before, those copies kept the files from the last install.

- A script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of a package that publishes from `publishConfig.directory`. Before, those copies kept the files from the last install.

- `verifyDepsBeforeRun` now compares a patch file that changed on disk with the patch hash in the lockfile. A patch that was copied or touched without changing its content no longer marks dependencies as outdated [#16988](https://github.com/pnpm/pnpm/issues/16988).

- `verifyDepsBeforeRun` now checks whether each importer's resolved catalog versions match the catalog snapshot in `pnpm-lock.yaml`.

  `verifyDepsBeforeRun` now respects `lockfileDir` in single-project repositories.

  When the install that runs before a script leaves `node_modules` out of date, pnpm now prints a warning.

- The `verifyDepsBeforeRun` check now notices an edited manifest, lockfile, or patch file even when the edit kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it.

  After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest in a project that install left out. A filtered command for a project that install did not reinstall after a lockfile change now installs first.

  `pnpm dedupe --check` and `pnpm install --dry-run` no longer record the dependencies as installed, so the `verifyDepsBeforeRun` check that follows them still notices outdated dependencies.

- `verifyDepsBeforeRun` now reports the dependencies as outdated when `sharedWorkspaceLockfile` is `false` and the `pnpm-lock.yaml` of a project the command runs is missing. Previously the check reported them as up to date, so `pnpm run` skipped the install [pnpm/tasks#138](https://github.com/pnpm/tasks/issues/138).

- `pnpm --version` now prints the version when a config file has a setting pnpm cannot read.

- `pnpm why` now reads each selected workspace project's lockfile and modules directory when `sharedWorkspaceLockfile` is `false` [pnpm/tasks#169](https://github.com/pnpm/tasks/issues/169).
