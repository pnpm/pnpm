## 12.15.0

This release lets `pnpm-workspace.yaml` delegate settings to other files and config dependencies with `include`, adds `lockfile.integrityTarget: tar` and tar-registry support, accepts zstd-compressed registry responses, and honors `SOURCE_DATE_EPOCH` in `pnpm deploy` and `pnpm install`. It also rejects registry metadata that names a different package than the one requested and speeds up workspace installs and macOS file linking.

### Minor Changes

- `pnpm-workspace.yaml` can now delegate settings to other YAML files and to config dependencies through a new `include` field. Each source may define only the settings the root manifest delegates to it. Every setting has one owner. Commands that change settings write each one to the file that owns it. A relative path in an included file is relative to that file. See [Splitting Settings Across Files](https://pnpm.io/config-includes).

  A config dependency supplies its settings through the YAML file named in the `pnpm.config` field of its `package.json`. The new `bootConfig` setting sets the release-age and trust policy pnpm uses to install config dependencies and the pnpm version from `packageManager`.

- Added the `lockfile.integrityTarget` setting. With `integrityTarget: tar`, `pnpm-lock.yaml` pins the hash of every package's uncompressed tar, so the lockfile no longer changes when a registry recompresses its tarballs or serves them as tars. pnpm learns a version's tar hash from the store or by downloading it once. `pnpm audit signatures` ties the tar hash to the signed `.tgz` integrity.

  ```yaml title="pnpm-workspace.yaml"
  lockfile:
    integrityTarget: tar
  ```

- pnpm can now install from a registry that serves uncompressed tars with zstd or gzip content coding. `pnpm-lock.yaml` records the hash of each tar. Declare such a registry with `serverType: pnpr` in `registries` to keep tarball URLs out of the lockfile. See [Tar registries](https://pnpm.io/registries#tar-registries).

  pnpm now decodes the content coding of tarball downloads itself. It rejects a zstd response whose window is larger than 8 MiB.

  Added the `maxTarballUnpackedSize` setting. It caps the bytes the files of one package may unpack to, 4 GiB by default.

- Registry revisions now always pin the hash of the uncompressed tar, the original included. pnpm records an original selected through the revision protocol, for example with a `+r0` specifier, as `revision: 0` in the lockfile and fetches it from the registry's digest route. pnpm checks a revision's tar whether the registry sends it as a tar, gzip, or zstd. Revision metadata must list revision 0 and mark the selected revision. pnpm rejects incomplete revision metadata with `ERR_PNPM_MALFORMED_METADATA`.

- pnpm now accepts zstd-compressed responses. Registries that serve zstd send package metadata in fewer bytes than with gzip, most of all the full metadata that settings such as `minimumReleaseAge` and `trustPolicy` read.

- `pnpm deploy` and `pnpm install` now use the `SOURCE_DATE_EPOCH` environment variable as an upper bound for timestamps in `node_modules/.modules.yaml` and `node_modules/.pnpm-workspace-state-v1.json`. With an equivalently prepared deploy target and an epoch no later than the timestamps that would otherwise be recorded, deploying the same project twice produces identical files [#17171](https://github.com/pnpm/pnpm/issues/17171).

- `pnpm update --interactive` now highlights the row under the cursor in cyan while keeping the colors of its version changes.

### Patch Changes

#### Security

- pnpm now fails with `ERR_PNPM_REGISTRY_NAME_MISMATCH` if a registry returns metadata for a package other than the one requested. Before, pnpm installed the package under the name in the metadata, so `allowBuilds` rules for the requested package did not apply to it.

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now keep exclusions for resolved configuration dependencies and their transitive dependencies [#17142](https://github.com/pnpm/pnpm/issues/17142).

#### Resolving dependencies

- When `pnpm install` resolves a dependency again, it now reuses a version that the install already resolved for the same package, as a fresh install does. Before, it kept the version the lockfile pinned and added a second copy of the package. A locked version still stays when nothing else in the install prefers another version. To pin a transitive dependency on purpose, use `overrides` [#17084](https://github.com/pnpm/pnpm/issues/17084).

- When another workspace project adds a different version of a package, a project's auto-installed peer on that package now moves to the version a fresh install picks. It used to stay on its locked version while any other project still accepted that version [#17058](https://github.com/pnpm/pnpm/issues/17058).

- A package that lists the same dependency in `dependencies` and as an optional peer now gets the peer bound when a project adds a provider for it later. The lockfile now records every peer a package declares, including one its own dependency supplies [#17056](https://github.com/pnpm/pnpm/issues/17056).

- `pnpm install --prod` and `pnpm deploy --prod` no longer drop a dependency that a package declares both as an optional peer and as its own dependency, when the package resolved it as its own dependency and a dev dependency provides the same package under another alias.

- `pnpm install` with an existing lockfile and a second `pnpm dedupe` now keep optional peer bindings that point into a dependency cycle or back into the dependent package. Before, the second run dropped the optional peer binding from the lockfile snapshot key.

- `pnpm install` and `pnpm dedupe` now preserve own-dependency and peer-dependency bindings when reusing a lockfile whose peer suffixes were hashed by `peersSuffixMaxLength`.

  Upgrading a direct dependency in one workspace project now refreshes matching transitive pins in sibling projects during the same `pnpm install` pass.

- With `resolvePeersFromWorkspaceRoot: false`, a workspace project no longer binds a peer to a version that only another project's dependencies provide. pnpm now auto-installs a version that satisfies the peer range for that project. Before, the project got a version outside the range, with no peer warning.

- `pnpm install`, `pnpm add`, and `pnpm update` now reuse the lockfile's resolution for dependencies that take part in a dependency cycle, such as `mlly` and `pkg-types`. They re-resolved those packages and everything that depends on them from the registry before [pnpm/tasks#130](https://github.com/pnpm/tasks/issues/130).

- pnpm now measures a `pnpm.overrides` entry written as a `workspace:` path, such as `workspace:./packages/foo`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote. The install then linked a directory that does not exist, or failed if workspace packages were injected [#17010](https://github.com/pnpm/pnpm/issues/17010).

- `pnpm install` now removes config dependencies that are no longer declared, including when the entire `configDependencies` setting is removed. Frozen installs reject a lockfile that still records removed config dependencies.

- `pnpm install --update-checksums` now re-resolves every locked package and records the integrity the registry reports now.

- `pnpm add` and `pnpm install` now accept `gist:<id>` dependencies and GitHub Gist URLs, and install them from the gist's archive. `pnpm pkg fix` normalizes a `gist:` repository the way `npm pkg fix` does, `pnpm licenses` links a gist package to its gist page, and `pnpm sbom` reports the repository of a package whose `repository` field is a `gist:` shorthand.

- `pnpm add` now reads the package name correctly from a tarball URL under a registry URL that has a path, such as `https://example.com/npm/`, and installs a registry tarball URL with a malformed percent escape as a plain tarball URL. Before, the registry path became part of the name, so a scoped package failed with `ERR_PNPM_INVALID_PACKAGE_NAME`.

- With `minimumReleaseAge` set, pnpm now revalidates a package's cached full metadata with a single ETag request so an unchanged registry answers with `304`, even when the registry does not list a publish time for every version. Previously, pnpm downloaded that metadata again whenever the cached copy was older than `minimumReleaseAge`. With `resolutionMode` set to `lowest-direct` or `time-based`, pnpm also reuses that metadata across every workspace project that depends on the package.

- When pnpm revalidates cached registry metadata, it now sends `If-Modified-Since` when the registry's modification time is a date without a time, and no longer crashes when the modification time is before 1970.

- Added protocol versioning between the pnpm client and pnpr server, verified applied resolution policies in resolution responses, and preserved structured error codes across the wire.

#### Performance

- Sped up `pnpm install` on macOS. On a 12-core Mac, installing 1352 packages from a warm store took 1.3 seconds, down from 2.2. pnpm now writes package files into `node_modules` with 4 threads on macOS, whatever the number of CPU cores. Set the `PNPM_MACOS_LINK_THREADS` environment variable to use a different number of threads, or to `0` to write files with the threads pnpm uses for the rest of the install.

- `pnpm install` is faster and uses less memory in large workspaces with `sharedWorkspaceLockfile: false`. Each project's install no longer reads the manifests of every other workspace project again. Installs that wait for their workspace dependencies now count toward `workspaceConcurrency` [#16966](https://github.com/pnpm/pnpm/issues/16966).

- Sped up workspace project discovery, dependency resolution, lockfile up-to-date checks, and peer dependency checks in large workspaces [#16839](https://github.com/pnpm/pnpm/issues/16839) [pnpm/tasks#9](https://github.com/pnpm/tasks/issues/9). On a workspace with 6,833 projects, the lockfile up-to-date check uses about 1 second less CPU time. The peer dependency check at the end of an install now checks linked workspace projects against the manifests already in memory, without reading each `package.json` again.

- Sped up bin linking and build planning during `pnpm install`. Package binary targets and shebang runtimes are now precalculated during tarball extraction and served in `pnpr` registry metadata alongside verified `requiresBuild`, tarball size, and file-count metadata.

- Lowered peak memory when pnpm downloads many packuments, such as during lockfile verification with `trustPolicy` on Linux. In one 1,800-entry workspace, a lockfile update now peaks at 1.8 to 2.0 GB. It peaked at 2.6 to 3.7 GB before [#17020](https://github.com/pnpm/pnpm/issues/17020).

- `pnpm install` in a Cargo workspace skips crates that an earlier install already unpacked into the store. Before, a repeat install processed every crate in `Cargo.lock` again. `pnpm install --force` unpacks every crate again [pnpm/tasks#69](https://github.com/pnpm/tasks/issues/69).

#### Linking node_modules and workspaces

- Concurrent installs that import the same package into one project now all complete. One of them used to fail with `ENOTEMPTY` when its copy of the package collided with the copy another install had already placed [pnpm/tasks#48](https://github.com/pnpm/tasks/issues/48).

- `pnpm install` now succeeds when `node_modules` is a symlink to an existing directory. Dependency hoisting previously rejected a symlinked `node_modules` as an invalid destination [#17157](https://github.com/pnpm/pnpm/issues/17157).

- A filtered first install under the isolated linker now links every workspace project by name into `node_modules/.pnpm/node_modules`, not only the selected ones [pnpm/tasks#155](https://github.com/pnpm/tasks/issues/155).

- With `nodeLinker: hoisted`, a filtered install such as `pnpm --filter app add <pkg>` keeps the root `node_modules` links and bins of the workspace projects it did not select. A project with no dependencies of its own lost them until the next full install.

- `pnpm install --filter` with `nodeLinker: hoisted` and `--prod` or `--dev` now preserves previously installed dependency groups for unselected workspace projects [pnpm/tasks#164](https://github.com/pnpm/tasks/issues/164). Repeat filtered installs also re-resolve when a transitive injected workspace dependency or a linked dependency's version changes.

- With `sharedWorkspaceLockfile: false`, `pnpm install` now runs the workspace root's pnpmfile for every project, after the project's own pnpmfile. Before, it ran only the pnpmfile in the project's directory.

- `pnpm install` and `pnpm peers check` now detect peer dependency issues and report linked package versions in linked projects that use `package.json5` or `package.yaml` [pnpm/tasks#172](https://github.com/pnpm/tasks/issues/172).

- `pnpm install` and `pnpm peers check` no longer report an unmet peer when a workspace package's peer is provided by a remote tarball URL. pnpm now compares the peer range with the version in the tarball's manifest [#17037](https://github.com/pnpm/pnpm/issues/17037).

- `pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also honors `--filter`, `--recursive`, `sharedWorkspaceLockfile: false`, and the `trustLockfile` setting and `--trust-lockfile` flag. With the lockfile trusted, `pnpm prune --prod` works offline when every package is in the store [#16999](https://github.com/pnpm/pnpm/issues/16999).

#### Injected workspace packages

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a local directory dependency or injected workspace package changes a dependency to a different tarball URL or git commit [#17129](https://github.com/pnpm/pnpm/issues/17129). It also fails when an injected workspace package changes a dependency's named-registry range, such as `work:^2.0.0`, to one the locked version no longer satisfies.

- `pnpm install --frozen-lockfile` now accepts the lockfile it wrote for a `file:` dependency on a directory without a `package.json`. It also accepts the lockfile for an injected workspace package that bundles a dependency in `bundledDependencies`, lists a dependency in both `dependencies` and `optionalDependencies`, or publishes from a `publishConfig.directory` whose `package.json` lists other dependencies than the project's own. Before, these failed with `ERR_PNPM_OUTDATED_LOCKFILE`.

- The lockfile now records a `catalog:` peer dependency of an injected workspace package as written in its `package.json`. It recorded the catalog's range before, so editing the catalog changed the consumer's lockfile entry [pnpm/tasks#153](https://github.com/pnpm/tasks/issues/153).

- `pnpm install` now resolves a git, tarball, or tag dependency of an injected workspace package again when its spec changes. Before, it kept the old version.

- Repeat installs and `verifyDepsBeforeRun` now re-check projects that inject a modified workspace package, including when `sharedWorkspaceLockfile` is `false` [#17007](https://github.com/pnpm/pnpm/issues/17007), and accept aliased `workspace:` ranges on linked dependencies.

- An injected workspace package that was linked now becomes a copy when the project that depends on it starts providing one of its peers at a version the package's own workspace context lacks. The link used to stay until that dependency was updated, so the package could not find the peer the project provides [#17057](https://github.com/pnpm/pnpm/issues/17057).

- An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.

- A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them, and updates those copies when `sharedWorkspaceLockfile` is `false`. Before, a directory link the script added became a real directory or went missing with `deployAllFiles`, and with `sharedWorkspaceLockfile: false` the copies kept the files from the last install.

#### Running scripts and checking dependencies

- Pressing Ctrl+C during `pnpm -r run` or `pnpm --filter <pkg> run` no longer reports `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL`. pnpm now ends with the interrupted script's status, such as 130 for a script that SIGINT ended on Linux or macOS [#16964](https://github.com/pnpm/pnpm/issues/16964).

- With `enableGlobalVirtualStore`, a Node.js resolve hook that a script registers now sees the importer as `context.parentURL` after pnpm's `NODE_PATH` fallback fails to find a bare specifier. Hooks that resolve tsconfig `paths` aliases from the importer failed with `ERR_MODULE_NOT_FOUND` [#16979](https://github.com/pnpm/pnpm/issues/16979).

- `pnpm run`, `pnpm exec`, and `pnpm restart` now pass the install flags they were given, such as `--lockfile-dir`, `--modules-dir`, `--store-dir`, `--offline`, and `--ignore-scripts`, to the `pnpm install` that `verifyDepsBeforeRun: install` starts. When that install leaves the dependencies out of date, pnpm now prints a warning.

- A repeated `pnpm install --filter` now finishes without reinstalling when the selected projects are up to date, and so does `pnpm install` once filtered commands have installed every project. After a filtered install, `pnpm run` and `pnpm exec` now install a selected project whose dependencies changed in between [#17011](https://github.com/pnpm/pnpm/issues/17011), require the workspace root when a filtered install with `pruneDirectDependencies: true` skipped it [pnpm/tasks#181](https://github.com/pnpm/tasks/issues/181), and preserve the recorded ignored build scripts of unselected projects.

- The check that runs before a script now holds the workspace projects that the selected project depends on, including with `sharedWorkspaceLockfile: false` and for workspace dependencies declared with `link:`. `--filter=<project>...` now selects `link:` dependencies as well. In a workspace that has no `package.json` at its root, `pnpm run` and `pnpm exec` no longer reinstall before every command when the dependencies are up to date.

- `verifyDepsBeforeRun` and repeat `pnpm install` runs now apply `overrides` before comparing a modified `package.json` with `pnpm-lock.yaml` [#17127](https://github.com/pnpm/pnpm/issues/17127). `pnpm install` with `optimisticRepeatInstall` now re-runs when `configDependencies` changed, `verifyDepsBeforeRun` reports `node_modules` out of date when `useGitBranchLockfile` is enabled and the branch lockfile is missing, and in a single-project repository both checks record a successful lockfile check so later runs skip it until the lockfile changes [#16993](https://github.com/pnpm/pnpm/issues/16993).

- `pnpm install` and `verifyDepsBeforeRun` now compare manifests, lockfiles, and patch files by content hash. They notice an edit that kept the file's modification time, as `cp -p`, `rsync -a`, or an archive extraction leave it, and a patch file that was copied or touched without changing its content no longer marks dependencies as outdated [#16988](https://github.com/pnpm/pnpm/issues/16988).

#### Deploying, packing, and other commands

- `pnpm deploy` now deploys `file:` dependencies that live outside the workspace directory. It failed with "Refusing to deploy unsafe lockfile path" [#16983](https://github.com/pnpm/pnpm/issues/16983).

- `pnpm deploy` now records the peer dependencies of the workspace packages it copies into the deployed lockfile. Before, pnpm rejected that lockfile with `--frozen-lockfile` when a deployed workspace package declared a peer and `injectWorkspacePackages` was off [#17093](https://github.com/pnpm/pnpm/issues/17093).

- `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` now succeed inside a directory created by `pnpm deploy`. `pnpm deploy` writes self-contained workspace and lockfile settings, prunes unused `patchedDependencies`, and preserves workspace-level dependency resolutions on deployed directory dependencies.

- On macOS, `pnpm deploy` no longer writes a second, unused copy of every package to `node_modules/.pnpm/@` in the deploy directory [#17116](https://github.com/pnpm/pnpm/issues/17116).

- `pnpm patch-commit` and `pnpm install` now apply a patch that renames or moves a file of the patched package. Such a patch failed with "rename/copy operations in patches are not yet supported".

- In workspaces with `sharedWorkspaceLockfile: false`, `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` checks configured patches across all workspace projects before reporting unused patches.

- `pnpm pack` and `pnpm publish` no longer include editor swap files such as `.*.swp`, macOS `._*` files, `.lock-wscript`, `.wafpickle-*`, `build/config.gypi`, or `archived-packages/` in the tarball. Naming one of these files in `files` still includes it [#16955](https://github.com/pnpm/pnpm/issues/16955).

- `pnpm pack` and `pnpm publish` now support extended glob patterns and brace ranges such as `part{1..3}.js` and `part{a..c}.js`, including descending ranges, steps, and zero padding, in `files`, `.npmignore`, and `.gitignore`. Explicitly listed internal directory symlinks are preserved below excluded directories, entry points honor nested ignore files, ignore negations can re-include default-excluded logs and backup files, and nested lockfiles follow the package's file selection rules.

- Trusted publishing with OIDC now works with registries whose URL has a path, such as `https://registry.example/npm/private/`. pnpm previously sent the token exchange request to the root of the registry host.

- `pnpm deprecate` and `pnpm unpublish` now work for packages whose registry metadata is larger than 10 MiB, such as `typescript`. They accept up to 50 MiB, the same limit as `pnpm owner`.

- `pnpm owner ls` now lists the owners of a package on the npm registry. It reported every package as not found.

- `pnpm licenses list --prod` now includes optional dependencies, including transitive ones. Pass `--no-optional` to leave them out. `pnpm licenses list` also reports no licenses for workspace projects not recorded in the lockfile, errors when `pnpm-lock.yaml` is missing, and formats empty results as the requested text or JSON [pnpm/tasks#174](https://github.com/pnpm/tasks/issues/174).

- `pnpm why` now reads each selected workspace project's lockfile and modules directory when `sharedWorkspaceLockfile` is `false` [pnpm/tasks#169](https://github.com/pnpm/tasks/issues/169).

- `pnpm pkg fix` now converts a `repository` string such as `github:pnpm/example` to the object form, as `npm pkg fix` does. It also normalizes a GitHub, GitLab, or Bitbucket repository URL the way `npm pkg fix` does [#16980](https://github.com/pnpm/pnpm/issues/16980).

#### Configuration and setting up pnpm

- Registry credentials and TLS settings passed with `--config`, such as `--config.//registry.example.com/:_authToken=<token>`, are now applied. They take precedence over the same settings in `.npmrc` files.

- pnpm now reads only the settings it needs to decide whether to switch to the pnpm version a project pins. A setting outside them can no longer fail that decision or change its outcome. For example, `pnpm --version` no longer fails in a pinned project whose `pnpm-workspace.yaml` has a setting this pnpm cannot read. Projects that pin neither pnpm nor a runtime skip this check without reading any settings.

- When pnpm switches to the version a project pins, an option only the pinned pnpm knows now reaches that pnpm when it comes immediately before the command name, as in `pnpm --new-flag install`. When switching for `pnpm --version` or an unrecognized command line, pnpm also honors `--color`, `--no-color`, `--reporter`, and `--loglevel` wherever they appear on the command line.

- `pnpm self-update` now updates the project's pnpm pin when Homebrew installed pnpm or Corepack runs it, leaving the global pnpm alone [#17001](https://github.com/pnpm/pnpm/issues/17001). Run inside a workspace package, `pnpm self-update` updates the pin in the workspace root's manifest, including in `package.yaml` or `package.json5`.

- `pnpm setup` now completes when Windows registry environment values cannot be read or written. It prints PowerShell commands to configure `PNPM_HOME` and `PATH` without registry access [#17167](https://github.com/pnpm/pnpm/issues/17167).

- `pnpm setup` now succeeds when it cannot tell which shell configuration file to update, for example in a container where `SHELL` is not set. It installs pnpm and prints the `PNPM_HOME` and `PATH` lines to add yourself. Previously it failed with `ERR_PNPM_UNKNOWN_SHELL`, and so did the installer script at get.pnpm.io [#16973](https://github.com/pnpm/pnpm/issues/16973).

- In StackBlitz WebContainers, `pnpm` no longer crashes intermittently with `Parking not supported on this platform`, and running a command that does not exist now reports that the command was not found. Previously it sometimes failed with a generic error.
