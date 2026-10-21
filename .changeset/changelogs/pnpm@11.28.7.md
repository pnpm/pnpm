## 11.28.7

This release rejects registry metadata that names a different package than the one requested, applies `trustPolicy` to the pnpm version a project pins, and caps unpacked package size with `maxTarballUnpackedSize`. It also removes support for registry revisions, fixes `verifyDepsBeforeRun` after filtered installs, and finishes commands with their own exit code when a pipe reader exits early.

### Patch Changes

#### Security

- pnpm now fails with `ERR_PNPM_REGISTRY_NAME_MISMATCH` if a registry returns metadata for a package other than the one requested. Before, pnpm installed the package under the name in the metadata, so `allowBuilds` rules for the requested package did not apply to it.

- pnpm now applies `trustPolicy` to the pnpm version a project pins in `packageManager` or `devEngines.packageManager`. A pinned version whose trust evidence was downgraded fails the command before pnpm switches to it or records it in the lockfile [#16319](https://github.com/pnpm/pnpm/issues/16319).

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now keep exclusions for resolved configuration dependencies and their transitive dependencies [#17142](https://github.com/pnpm/pnpm/issues/17142).

- Added the `maxTarballUnpackedSize` setting. `pnpm install` fails with `ERR_PNPM_TARBALL_TOO_LARGE` when the files of one package unpack to more than this many bytes, 4 GiB by default [pnpm/tasks#81](https://github.com/pnpm/tasks/issues/81).

#### Resolving dependencies

- Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed.

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- When another workspace project adds a different version of a package, a project's auto-installed peer on that package now moves to the version a fresh install picks. It used to stay on its locked version while any other project still accepted that version [#17058](https://github.com/pnpm/pnpm/issues/17058).

- An optional peer that pnpm adds to a project now keeps the version the lockfile recorded for that project only while the dependency graph still holds it. Otherwise it follows its provider to a version the graph holds. Before, pnpm kept a higher version that only the lockfile held, which left two copies of the peer [#16443](https://github.com/pnpm/pnpm/issues/16443).

- A package that lists the same dependency in `dependencies` and as an optional peer now gets the peer bound when a project adds a provider for it later. The lockfile now records every peer a package declares, including one its own dependency supplies [#17056](https://github.com/pnpm/pnpm/issues/17056).

- `pnpm install --prod` no longer drops a dependency that a package declares both as an optional peer and as its own dependency, when the package resolved it as its own dependency and a dev dependency provides the same package under another alias.

- pnpm now measures a `pnpm.overrides` entry written as a `workspace:` path, such as `workspace:./packages/foo`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote. The install then linked a directory that does not exist, or failed if workspace packages were injected [#17010](https://github.com/pnpm/pnpm/issues/17010).

- `pnpm install` now removes config dependencies that are no longer declared, including when the entire `configDependencies` setting is removed. Frozen installs reject a lockfile that still records removed config dependencies.

- `pnpm add` now reads the package name correctly from a tarball URL under a registry URL that has a path, such as `https://example.com/npm/`. Before, the registry path became part of the name, so a scoped package failed with `ERR_PNPM_INVALID_PACKAGE_NAME`.

- `pnpm add` no longer warns that a gist dependency was replaced from a different source when the new specifier names the same gist in another form, such as `gist:<id>` and `https://gist.github.com/<user>/<id>`.

- With `minimumReleaseAge` set, pnpm now revalidates a package's cached full metadata with a single ETag request so an unchanged registry answers with `304`, and keeps using the cached copy once the registry confirms it is current, even when the metadata does not list a publish time for every version. Previously, pnpm downloaded that metadata again whenever the cached copy was older than `minimumReleaseAge`. Packages listed in `minimumReleaseAgeExclude` also resolve from cached metadata without a registry request when the lockfile already has the version their range picks.

#### Linking node_modules and workspaces

- Concurrent installs that import the same package into one project now all complete. One of them used to fail with `ENOTEMPTY` when its copy of the package collided with the copy another install had already placed [pnpm/tasks#48](https://github.com/pnpm/tasks/issues/48).

- `pnpm install` no longer fails on macOS with `EISDIR` when it creates store directories in a group-writable store under a umask that removes owner read, such as `0o477`.

- `pnpm install` now succeeds when `node_modules` is a symlink to an existing directory. Dependency hoisting previously rejected a symlinked `node_modules` as an invalid destination [#17157](https://github.com/pnpm/pnpm/issues/17157).

- A filtered `pnpm install --no-lockfile` with `hoistWorkspacePackages` now hoists every workspace project into `node_modules/.pnpm/node_modules`. It used to hoist only the selected projects [pnpm/tasks#168](https://github.com/pnpm/tasks/issues/168).

- Filtered installs remove obsolete dependencies from the installed lockfile while preserving dependencies still needed by unselected projects.

- With `sharedWorkspaceLockfile: false`, `pnpm install` now runs the `preResolution` hooks and custom resolvers and fetchers of the workspace root's pnpmfile for every project, next to the project's own. A global pnpmfile now runs once per project, and each project's lockfile records the checksum of the root pnpmfile when the project has none of its own. Before, a project's own pnpmfile replaced these root hooks, the global hooks ran twice, and edits to the root pnpmfile could go unnoticed.

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also accepts `--filter` and `--recursive` to prune selected workspace projects without removing dependencies of unselected projects.

#### Injected workspace packages

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of an injected workspace package or local directory dependency no longer match the lockfile. This covers `sharedWorkspaceLockfile: false` [#17008](https://github.com/pnpm/pnpm/issues/17008), a dependency changed to a different tarball URL or git commit [#17129](https://github.com/pnpm/pnpm/issues/17129), and a change in a transitive injected or local directory dependency [#17128](https://github.com/pnpm/pnpm/issues/17128).

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when an injected workspace package depends on a local directory under another name, such as `"renamed": "file:../b"` [#17130](https://github.com/pnpm/pnpm/issues/17130).

- An injected workspace dependency now becomes a link when its dependencies match the workspace package's own, also on an install that leaves the package's installed dependencies as they are. Before, such an install kept it as a copy [#17094](https://github.com/pnpm/pnpm/issues/17094).

- An injected workspace package that was linked now becomes a copy when the project that depends on it starts providing one of its peers at a version the package's own workspace context lacks. The link used to stay until that dependency was updated, so the package could not find the peer the project provides [#17057](https://github.com/pnpm/pnpm/issues/17057).

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them. It no longer fails when the package links to one directory twice or links back to a directory above the link. `pnpm install` with `deployAllFiles` no longer fails on such a link back either.

- A script listed in `syncInjectedDepsAfterScripts` now updates the injected copies of the package it ran in when `sharedWorkspaceLockfile` is `false` or the package publishes from `publishConfig.directory`. Before, those copies kept the files from the last install.

#### Running scripts and checking dependencies

- When the reader of pnpm's output exits early, as in `pnpm outdated | head`, pnpm now finishes the command and exits with the command's own exit code [pnpm/tasks#133](https://github.com/pnpm/tasks/issues/133). It used to exit with code 0 at the first write to the closed pipe. That hid the result of commands such as `pnpm outdated` and stopped `pnpm clean` before it removed anything.

- With `enableGlobalVirtualStore`, a Node.js resolve hook that a script registers now sees the importer as `context.parentURL` after pnpm's `NODE_PATH` fallback fails to find a bare specifier. Hooks that resolve tsconfig `paths` aliases from the importer failed with `ERR_MODULE_NOT_FOUND` [#16979](https://github.com/pnpm/pnpm/issues/16979).

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. When that install leaves `node_modules` out of date, pnpm now prints a warning.

- After `pnpm install --filter`, `pnpm run` and `pnpm exec` now notice a changed manifest or lockfile in a project that the filtered install left out, including the workspace root after a filtered install with `pruneDirectDependencies: true` [pnpm/tasks#181](https://github.com/pnpm/tasks/issues/181). A filtered install also preserves the recorded ignored build scripts of unselected projects, and `pnpm dedupe --check` and `pnpm install --dry-run` no longer record outdated dependencies as installed.

- The `verifyDepsBeforeRun` check now holds the workspace projects that the selected project depends on, including `link:` workspace dependencies. `--filter=<project>...` now selects `link:` dependencies as well. With `sharedWorkspaceLockfile: false`, `verifyDepsBeforeRun` also reports dependencies as outdated when a selected project's `pnpm-lock.yaml` is missing [pnpm/tasks#138](https://github.com/pnpm/tasks/issues/138).

- `verifyDepsBeforeRun` and repeat `pnpm install` runs now apply `overrides` before comparing a modified `package.json` with `pnpm-lock.yaml` [#17127](https://github.com/pnpm/pnpm/issues/17127). `verifyDepsBeforeRun` also checks whether each importer's resolved catalog versions match the catalog snapshot in `pnpm-lock.yaml`, and respects `lockfileDir` in single-project repositories.

- `verifyDepsBeforeRun` now compares manifests, lockfiles, and patch files by content hash. It notices an edit that kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it, and a patch file that was copied or touched without changing its content no longer marks dependencies as outdated [#16988](https://github.com/pnpm/pnpm/issues/16988).

#### Deploying and other commands

- `pnpm deploy` now records the peer dependencies of the workspace packages it copies into the deployed lockfile. Before, pnpm rejected that lockfile with `--frozen-lockfile` when a deployed workspace package declared a peer and `injectWorkspacePackages` was off [#17093](https://github.com/pnpm/pnpm/issues/17093).

- `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` now succeed inside a directory created by `pnpm deploy`. `pnpm deploy` writes self-contained workspace and lockfile settings, prunes unused `patchedDependencies`, and preserves workspace-level dependency resolutions on deployed directory dependencies.

- In workspaces with `sharedWorkspaceLockfile: false`, `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` checks configured patches across all workspace projects before reporting unused patches.

- Trusted publishing with OIDC now works with registries whose URL has a path, such as `https://registry.example/npm/private/`. pnpm previously sent the token exchange request to the root of the registry host.

- `pnpm owner ls` now lists the owners of a package on the npm registry. It reported every package as not found.

- `pnpm sbom` now reads project lockfiles in workspaces with `sharedWorkspaceLockfile: false`, including filtered and split output, and reports the repository of a package whose `repository` field is a `gist:` shorthand.

- `pnpm licenses list` reports no licenses for workspace projects not recorded in the lockfile. It errors when `pnpm-lock.yaml` is missing. Empty results now use the requested text or JSON format [pnpm/tasks#174](https://github.com/pnpm/tasks/issues/174).

- `pnpm why` now reads each selected workspace project's lockfile and modules directory when `sharedWorkspaceLockfile` is `false` [pnpm/tasks#169](https://github.com/pnpm/tasks/issues/169).

- `pnpm pkg fix` now converts a `repository` string such as `github:pnpm/example` to the object form, as `npm pkg fix` does. It also normalizes a GitHub, GitLab, or Bitbucket repository URL the way `npm pkg fix` does [#16980](https://github.com/pnpm/pnpm/issues/16980).

#### Configuration and setting up pnpm

- pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).

- pnpm now ignores a `PNPM_CONFIG_*` environment variable for a URL setting, such as `PNPM_CONFIG_REGISTRY`, when its value is not a URL. Such a value used to make every command fail with `Invalid URL`.

- The warning about unrecognized settings in `pnpm-workspace.yaml` and the global `config.yaml` now names `addMissingPeerTypes`, `autoDedupe`, `bootConfig`, `macosBackup`, `nodePackageMap`, `permissions`, `pipelineBase`, `publishWaitTimeout`, `python`, `remoteCache`, `saveTypes`, `skills`, `storeIndexFormat`, and `tools` as pnpm v12 settings [pnpm/tasks#167](https://github.com/pnpm/tasks/issues/167).

- When a config file has a setting pnpm cannot read, `pnpm --version` now prints the version, and `pmOnFail` and `lockfileDir` in `pnpm-workspace.yaml` stay in force. Previously, pnpm switched to the pinned version despite `pmOnFail: ignore`. pnpm also no longer writes the pinned version to `pnpm-lock.yaml` when it cannot read every setting and the running pnpm already matches the pin.

- pnpm now records the pinned pnpm version in the `pnpm-lock.yaml` under the `lockfileDir` set in `pnpm-workspace.yaml`, and resolves a relative `lockfileDir` from the workspace root. pnpm used to write a second `pnpm-lock.yaml` at the workspace root.

- `pnpm self-update` now updates the project's pnpm pin when Homebrew installed pnpm or Corepack runs it. The global pnpm is left alone [#17001](https://github.com/pnpm/pnpm/issues/17001).

- `pnpm setup` now completes when Windows registry environment values cannot be read or written. It prints PowerShell commands to configure `PNPM_HOME` and `PATH` without registry access [#17167](https://github.com/pnpm/pnpm/issues/17167).

- `pnpm setup` now succeeds when it cannot tell which shell configuration file to update, for example in a container where `SHELL` is not set. It installs pnpm and prints the `PNPM_HOME` and `PATH` lines to add yourself. Previously it failed with `ERR_PNPM_UNKNOWN_SHELL`, and so did the installer script at get.pnpm.io [#16973](https://github.com/pnpm/pnpm/issues/16973).
