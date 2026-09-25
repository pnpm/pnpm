## 12.7.0

### Minor Changes

- The global `node` shim created by pnpm now uses the Node.js version from the nearest `.nvmrc` file when the project does not declare a Node.js runtime in `devEngines.runtime` or `engines.runtime` [pnpm/pnpm#4471](https://github.com/pnpm/pnpm/issues/4471). An `.nvmrc` value that only nvm can act on, such as `system` or a custom alias, is ignored.

- `pnpm install --force` now keeps skipping optional dependencies whose `os`, `cpu` or `libc` do not match the host. It still refetches every package and lifts `engineStrict`. The new `forceIgnoresPlatform` setting restores the previous behaviour, installing optional dependencies of every platform under `--force` [#6133](https://github.com/pnpm/pnpm/issues/6133).

- `pnpm install` now supports the `--allow-build` option to selectively allow or deny package lifecycle scripts and record them in `pnpm-workspace.yaml` [pnpm/pnpm#15388](https://github.com/pnpm/pnpm/issues/15388).

- pnpm now keeps the blank lines between entries of `package.json` when it updates the file, for example on `pnpm add` [#5602](https://github.com/pnpm/pnpm/issues/5602).

- When a project pins a pnpm version or a runtime that another pnpm process is installing at that moment, pnpm now waits a few seconds and then installs and runs a private copy of its own. It used to wait up to five minutes and then use the shared install directory without the lock. The private copy is removed once the command has run. `pnpm store prune` removes any private copy that a killed process left behind [#15413](https://github.com/pnpm/pnpm/issues/15413).

- Added `pnpm publish --publish-wait-timeout <milliseconds>` to wait for published versions and their tarballs to become available from the registry. Set `publishWaitTimeout` in `pnpm-workspace.yaml` to configure a default. A value of `0` disables the check.

  Recursive publishing confirms availability before publishing dependent packages. If confirmation times out, the command fails.

  When `pnpm publish -r --report-summary` fails after some uploads were accepted, the summary file now lists those packages.

- The global `node` shim created by pnpm now reads `.node-version` files as well as `.nvmrc` files. The nearest directory with a Node.js runtime declaration decides the version. Within one directory, `package.json` takes precedence over `.node-version`, which takes precedence over `.nvmrc`.

- `pnpm install` now creates `pnpm-workspace.yaml` from the `workspaces` field of the root `package.json` when the repository has no `pnpm-workspace.yaml`. The projects the field lists are linked on that same install. An existing `pnpm-workspace.yaml` is never changed. With `--ignore-workspace`, no file is created. If the `workspaces` field later differs from `packages` in `pnpm-workspace.yaml`, pnpm prints a warning [#2255](https://github.com/pnpm/pnpm/issues/2255).

### Patch Changes

- `pnpm audit --fix` now prunes redundant overrides when one vulnerable range is a subset of another for the same package [#8577](https://github.com/pnpm/pnpm/issues/8577).

- `pnpm audit` now lists at least one dependency path from every workspace project that depends on a vulnerable package. Before, a project whose dependency was reached through more than 100 paths filled the path list, and other projects that depend on the same package were left out [#12200](https://github.com/pnpm/pnpm/issues/12200).

- `pnpm audit` and `pnpm audit signatures` now check only the dependencies of the projects selected by `--filter`, `--filter-prod`, or `--workspace-root`. The filter used to be ignored, so a filtered audit reported the whole workspace [#10982](https://github.com/pnpm/pnpm/issues/10982).

- `autoDedupe` and `pnpm dedupe` now move transitive dependencies to the version a `catalog:` dependency pins, as they already did for versions written directly in `package.json`. Previously they could move those dependencies to a higher version and keep both versions in the lockfile.

- Bash completion now completes script names that contain a colon, such as `pnpm run test:u` to `pnpm run test:unit` [pnpm/pnpm#5482](https://github.com/pnpm/pnpm/issues/5482).

- Fixed `pnpm install`, `pnpm add`, `pnpm remove`, and `pnpm peers check` running out of memory when many packages share a missing peer dependency. This mostly affected projects with `autoInstallPeers: false` [#15362](https://github.com/pnpm/pnpm/issues/15362).

- `pnpm install` now caps concurrent connections to a proxy at 50 sockets by default [pnpm/pnpm#15280](https://github.com/pnpm/pnpm/issues/15280). It also immediately retries transient connection resets when downloading package archives.

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` now move the catalog entry onto the named version when the entry's range already covers it. For example, `^7.22.17` becomes `^7.29.6`, the same way `pnpm update <pkg>` moves an entry to the version it resolves [#13715](https://github.com/pnpm/pnpm/issues/13715).

- A dependency declared with `catalog:` now counts as a workspace dependency when its catalog entry points at a workspace project, for example `workspace:*`. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it [#15587](https://github.com/pnpm/pnpm/issues/15587).

- pnpm now detects the same CI environments as pnpm 11, including AWS CodeBuild, which does not set `CI`. On these services `pnpm install` uses a frozen lockfile by default and fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the lockfile is outdated.

- Interrupting `pnpm install` with Ctrl+C or SIGTERM no longer leaves a temporary lockfile (`.pnpm-lock.yaml.*.tmp`) behind in the project [#1418](https://github.com/pnpm/pnpm/issues/1418).

- Honor the `reporter` setting when it is configured in `pnpm-workspace.yaml`, the global configuration, or the `PNPM_CONFIG_REPORTER` environment variable. Configured `reporter: silent` makes silent output the default. An explicit `--reporter` takes precedence [#4879](https://github.com/pnpm/pnpm/issues/4879).

- Custom fetcher hooks no longer run a second time during installation when an archive was already fetched during dependency resolution [pnpm/pnpm#15025](https://github.com/pnpm/pnpm/issues/15025).

- Fixed a package resolved by a `resolvers` pnpmfile hook installing without its own dependencies. This happened when the hook returned no `manifest` and a `fetchers` hook handled the resolution [#15552](https://github.com/pnpm/pnpm/issues/15552).

- `pnpm dedupe` now produces a stable lockfile when a dependency's range matches both a direct dependency and an `npm:` alias of the same package. The dependency resolves to the version of the direct dependency. Repeated runs previously alternated between two lockfiles [#15588](https://github.com/pnpm/pnpm/issues/15588).

- `pnpm dedupe` now counts each package reused from the store once in its progress output [#15303](https://github.com/pnpm/pnpm/issues/15303).

- `pnpm deploy` now copies the `packageManager` and `devEngines.packageManager` fields of the workspace root `package.json` into the deployed `package.json`, unless the deployed project pins a package manager itself [#9079](https://github.com/pnpm/pnpm/issues/9079).

- `pnpm deploy --legacy` no longer rewrites the source workspace's `node_modules/.pnpm-workspace-state-v1.json` to describe only the deployed project [#15352](https://github.com/pnpm/pnpm/issues/15352).

- `pnpm deploy` now respects `--package-import-method` passed on the command line and reports the package import method correctly [pnpm/pnpm#7593](https://github.com/pnpm/pnpm/issues/7593).

- `pnpm deploy` now puts the virtual store at `virtualStoreDir`, resolved against the deploy directory. A shared-lockfile deploy records `virtualStoreDir` in the deployed `pnpm-workspace.yaml`. With the global virtual store enabled or an absolute `virtualStoreDir`, the deploy still uses `node_modules/.pnpm` [#8787](https://github.com/pnpm/pnpm/issues/8787).

- `pnpm deploy` does not run the `prepare` scripts of the deployed project ([#7282](https://github.com/pnpm/pnpm/issues/7282)).

- `pnpm install` no longer skips optional dependencies that the Node.js version locked for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm dlx` now keeps a separate cache entry for each Node.js major version. A package built under one Node.js major version, such as a native addon, is no longer reused under another [#8611](https://github.com/pnpm/pnpm/issues/8611).

- Clean up dangling Node executables and shims during `pnpm env remove`. Surviving global commands remain intact.

- `pnpm publish` and `pnpm pack` now report an error when a bin script has a shebang line ending with CRLF [pnpm/pnpm#7311](https://github.com/pnpm/pnpm/issues/7311).

- `pnpm exec` and `pnpm dlx` now set `npm_execpath`, `INIT_CWD`, `npm_node_execpath`, and `NODE` in child environments when Node.js is available. Stale inherited `NODE` and `npm_node_execpath` variables are cleared when Node.js cannot be found on PATH [#7037](https://github.com/pnpm/pnpm/issues/7037).

- `pnpm exec` now sets the `PWD` environment variable to the directory the command runs in. Shells and tools that read `PWD` now report the logical path of a workspace package reached through a symlink [#1550](https://github.com/pnpm/pnpm/issues/1550).

- Requests to a registry or tarball server whose TLS certificate fails verification now fail at once. Such requests were retried for more than a minute without any output [#9134](https://github.com/pnpm/pnpm/issues/9134).

- Fixed installs failing on macOS when system trust evaluation is restricted, such as in a sandbox. Network requests now fall back to bundled CA roots when the platform trust service is unreachable, and honor custom `ca` certificates directly [#15329](https://github.com/pnpm/pnpm/issues/15329).

- A repeat `pnpm install` in a workspace with a custom `modulesDir` now takes the up-to-date fast path. Before, pnpm looked for each workspace project's dependencies in `node_modules` and ran a full install every time.

- `pnpm fetch` now also installs the pnpm version that `pnpm-lock.yaml` pins, when it differs from the running pnpm. A later `pnpm install --offline` that switches to the pinned version no longer fails because that version is missing from the store [#11808](https://github.com/pnpm/pnpm/issues/11808).

- `pnpm --filter "[<since>]"` now selects workspace packages when dependency versions change in a catalog in `pnpm-workspace.yaml` [#8718](https://github.com/pnpm/pnpm/issues/8718).

- `--filter "[<since>]"` now selects projects that files were moved out of when git detects the move as a rename [pnpm/pnpm#15481](https://github.com/pnpm/pnpm/issues/15481).

- `--filter` now evaluates selectors in order, so later inclusion filters can re-include packages that an earlier exclusion filter excluded [pnpm/pnpm#9354](https://github.com/pnpm/pnpm/issues/9354).

- `pnpm --filter <project> <command>` and `pnpm -r <command>` now run a command installed in the selected projects' dependencies when none of them has a script by that name. This matches `pnpm <command>` in a single project. `pnpm run` with `--filter` or `-r` still reports the missing script [#10151](https://github.com/pnpm/pnpm/issues/10151).

- The `[<since>]` filter selector now compares against the commit where the current branch forked from `<since>`. Projects changed only by newer commits on `<since>` are no longer selected. Uncommitted changes are still included. In a shallow clone without that commit, pnpm compares against `<since>` directly, as before [#9907](https://github.com/pnpm/pnpm/issues/9907).

- `pnpm remove` now accepts `--trust-lockfile` and `--no-trust-lockfile` to control supply-chain policy checks while removing a package [#14406](https://github.com/pnpm/pnpm/issues/14406).

- `pnpm install` now preserves existing `node_modules` directories when a cross-device move reports `EXDEV` [pnpm/pnpm#14504](https://github.com/pnpm/pnpm/issues/14504).

- `pnpm install` in WSL now waits out Windows file locks on a Windows drive such as `/mnt/c`, as it already does on Windows. Before, an antivirus or indexer scan holding a file open could fail the install with `EACCES` [pnpm/pnpm#6155](https://github.com/pnpm/pnpm/issues/6155).

- `pnpm add` now saves changes to `package.json` before running lifecycle scripts, so a postinstall script failure leaves the added dependency in `package.json` [#8627](https://github.com/pnpm/pnpm/issues/8627).

- `pnpm add` now saves the requested exact version when adding a dependency, even when the manifest already contains a version range [#6040](https://github.com/pnpm/pnpm/issues/6040).

- Removal overrides such as `"parent>peer": "-"` now prevent optional peers from being installed from another workspace package [#15008](https://github.com/pnpm/pnpm/issues/15008).

- Fixed `pnpm version` failing on projects using a `package.yaml` manifest.

  Fixed `pnpm init` creating an extra `package.json` when `package.yaml` is already present.

- `pnpm sbom` filtered to a single workspace project now takes the `author`, `description`, `license`, `repository`, and `bugs` fields from the workspace root `package.json` when the project does not declare them. A field the project declares is never taken from the root, even when it is blank or `null` [#14882](https://github.com/pnpm/pnpm/issues/14882).

- `pnpm self-update` run in a project that pins pnpm through `packageManager` or `devEngines.packageManager` now also updates the global pnpm, as it does outside a project [#14747](https://github.com/pnpm/pnpm/issues/14747).

- The `publish` command now resolves `workspace:` dependencies from workspace manifests when `node_modules` is not installed. Previously, publishing without `node_modules` failed with `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL`. pnpm/pnpm#6567

- `pnpm install --frozen-lockfile` now rejects changed local tarballs, even when the previous archive contents are in the store [pnpm/pnpm#1889](https://github.com/pnpm/pnpm/issues/1889).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when `pnpm-lock.yaml` lists a workspace project whose directory or manifest file is missing. The install used to report success without installing that project's dependencies [#7667](https://github.com/pnpm/pnpm/issues/7667).

- `pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. `minimumReleaseAgeExclude` entries match action names such as `actions/checkout` [#13923](https://github.com/pnpm/pnpm/issues/13923).

- Global commands such as `pnpm add --global`, `pnpm list --global`, and `pnpm bin --global` now run with the pnpm you invoked, even in a project that pins another pnpm version. Previously, a pin with `onFail: "download"` switched them to the pinned pnpm, and a pinned pnpm 10 or older failed because its global bin directory was not in `PATH` [#14531](https://github.com/pnpm/pnpm/issues/14531).

- `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` no longer fail with `ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR` when another global package's link into the store dangles, for example after the store was pruned. The pnpm install script failed the same way on such a machine.

- `pnpm update --global` now skips a global package installed from a `file:` path that no longer exists, prints a warning, and updates the remaining global packages. Previously the whole update failed with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` [#12533](https://github.com/pnpm/pnpm/issues/12533).

- A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".

- `pnpm install` with `--filter` now installs only the dependencies of the selected projects when using `nodeLinker: hoisted` [#8882](https://github.com/pnpm/pnpm/issues/8882).

- A repeat `pnpm install --frozen-lockfile` with `nodeLinker: hoisted` in a workspace no longer re-links `node_modules` when nothing changed.

- With `nodeLinker: hoisted`, pnpm now installs the root project's dependencies into a custom `modulesDir` instead of `node_modules`. With a custom `modulesDir`, the virtual store and its `lock.yaml` now default to `<modulesDir>/.pnpm`.

- With `nodeLinker: hoisted`, `hoistWorkspacePackages` now links each workspace project that `hoistPattern` or `publicHoistPattern` selects into the root `node_modules`, unless a hoisted package or a root dependency already uses its name. The project's bins are linked into the root `node_modules/.bin` [#7553](https://github.com/pnpm/pnpm/issues/7553).

- With `nodeLinker: hoisted`, `pnpm install` now removes the commands of the packages it removes from `node_modules/.bin`, such as a nested copy deduped into the root `node_modules` [#7568](https://github.com/pnpm/pnpm/issues/7568).

- `pnpm install --ignore-pnpmfile` no longer removes `pnpmfileChecksum` from an up-to-date `pnpm-lock.yaml`. `pnpm install --frozen-lockfile --ignore-pnpmfile` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the lockfile records a `pnpmfileChecksum`. A command that resolves dependencies with the pnpmfile ignored still writes the lockfile without it [#10944](https://github.com/pnpm/pnpm/issues/10944).

- `pnpm import` now converts dependencies that use Yarn's `patch:` protocol. The dependency keeps the version it patches, and the patch file is added to `patchedDependencies` in `pnpm-workspace.yaml`. If the patch file is missing, pnpm prints a warning and imports the dependency without the patch [#10278](https://github.com/pnpm/pnpm/issues/10278).

- `pnpm import` in a workspace now keeps the versions pinned by the root `yarn.lock`, `package-lock.json`, or `npm-shrinkwrap.json` when another workspace project's range allows a newer version. Before, the root project got the newest version in its range [#4385](https://github.com/pnpm/pnpm/issues/4385).

- `pnpm import` now warns when package.json lists projects in a "workspaces" array and there is no "pnpm-workspace.yaml". Without that file, the import writes a lockfile for the root project only [#5240](https://github.com/pnpm/pnpm/issues/5240).

- `pnpm init` now supports the `--bare` option. It creates a `package.json` file with only the required fields [pnpm/pnpm#15538](https://github.com/pnpm/pnpm/issues/15538).

- Packages that run a lifecycle script are no longer hard-linked into the virtual store, so a build script can no longer rewrite the workspace source of an injected package or the store copy it was imported from [pnpm/pnpm#15483](https://github.com/pnpm/pnpm/issues/15483).

- `syncInjectedDepsAfterScripts` now copies files into injected dependencies when `node_modules` is on another filesystem than the package sources. The sync previously failed with a cross-device link error and made the script run exit with an error [pnpm/pnpm#14703](https://github.com/pnpm/pnpm/issues/14703).

- Allow external dependencies to be updated when running `pnpm update --interactive` with `--workspace`.

- Restored reading and updating `package.json5` project manifests. Manifest updates retain comments, and workspace discovery prefers `package.json`, then `package.json5`, then `package.yaml` [pnpm/pnpm#15129](https://github.com/pnpm/pnpm/issues/15129).

- `pnpm add` and `pnpm install` keep an empty `peerDependencies`, `dependencies`, `devDependencies`, or `optionalDependencies` field that was already in `package.json`. pnpm still drops such a field when it removes the last entry itself, as `pnpm remove` does [#5096](https://github.com/pnpm/pnpm/issues/5096).

- A script that runs `pnpm run` no longer adds duplicate `node_modules/.bin` and `node-gyp-bin` entries to `PATH` [#5352](https://github.com/pnpm/pnpm/issues/5352).

- `pnpm licenses list` run inside a workspace project now lists only that project's dependencies. Use `--recursive` or `--filter` to list the licenses of other workspace projects [#5689](https://github.com/pnpm/pnpm/issues/5689).

- `pnpm licenses list` failed or reported nothing in a workspace with `sharedWorkspaceLockfile: false`. It now reads the lockfile of each selected project [#10140](https://github.com/pnpm/pnpm/issues/10140).

- With `nodeLinker: hoisted`, `pnpm licenses list` reported every license as `Unknown` and listed paths under `node_modules/.pnpm` that do not exist. It now reads each package from the directory where the hoisted linker placed it [#8589](https://github.com/pnpm/pnpm/issues/8589).

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- `pnpm install` uses less CPU when it links packages from a warm store. On Windows, a warm install could take several times longer than with pnpm 11 [#15439](https://github.com/pnpm/pnpm/issues/15439).

- `pnpm list --only-projects` now lists the workspace projects when `sharedWorkspaceLockfile` is `false` [#7151](https://github.com/pnpm/pnpm/issues/7151).

- `pnpm list --only-projects` now lists a workspace project that sets `publishConfig.directory`. Dependents link such a project through its publish directory, which `--only-projects` did not recognize as a project [#10635](https://github.com/pnpm/pnpm/issues/10635).

- `pnpm list --only-projects` now prints every project selected with `--filter` or `--recursive`, including a project that has no workspace dependencies [#9770](https://github.com/pnpm/pnpm/issues/9770).

  `pnpm list --only-projects` no longer reports packages in `node_modules` that are missing from the lockfile [#9528](https://github.com/pnpm/pnpm/issues/9528).

- Running `pnpm list` inside a workspace package without `--recursive` or a filter now lists only the current package [pnpm/pnpm#14494](https://github.com/pnpm/pnpm/issues/14494).

- `pnpm install` and `pnpm peers check` now use local tarball packages' actual versions when checking peer dependencies. Compatible packages no longer fail with `strictPeerDependencies` enabled.

- On Windows, the error for a `node_modules` directory that pnpm cannot move out of the way now names the directory and says that a file in it is probably in use by another process [#7505](https://github.com/pnpm/pnpm/issues/7505).

- On Windows, pnpm now retries saving `pnpm-lock.yaml` for up to a minute while another process holds the file open. The save used to fail at once with `EPERM`, `EBUSY`, or "Access is denied" [#9461](https://github.com/pnpm/pnpm/issues/9461).

- `pnpm login` now logs back in to an existing user on registries without web login, such as verdaccio. The classic login request sends the username and password as basic auth, as `npm login` does [#12055](https://github.com/pnpm/pnpm/issues/12055).

- On macOS, `pnpm` now uses its bundled certificate roots when macOS cannot create an SSL policy for a registry connection. It crashed on the first registry request in that case [pnpm/pnpm#14461](https://github.com/pnpm/pnpm/issues/14461).

- When pnpm switches to the version a project pins, the `minimumReleaseAge` approvals for that version are now added to `minimumReleaseAgeExclude` in the project's `pnpm-workspace.yaml`. A project without that file gets one. Global commands leave the project's settings unchanged [#15396](https://github.com/pnpm/pnpm/issues/15396).

- When the configured `scriptShell` does not exist, running a script now fails with an error that names the shell. Previously pnpm printed only an exit code or the package directory [#7562](https://github.com/pnpm/pnpm/issues/7562).

- `pnpm run` and `pnpm exec` with `verifyDepsBeforeRun` now accept a moved project whose store is on the project's volume. Before, the check reported that the workspace structure had changed whenever the default store was not on the home volume.

- `pnpm peers check` and the `ERR_PNPM_PEER_DEP_ISSUES` error now group peer dependency issues under the workspace project they were found in [#15351](https://github.com/pnpm/pnpm/issues/15351).

- A `modulesDir` with several path segments, such as `www/modules`, now puts each workspace project's dependencies in `<project>/www/modules` on both fresh and frozen installs, and `pnpm bin` prints `<project>/www/modules/.bin` [#15484](https://github.com/pnpm/pnpm/issues/15484).

- `pnpm install --no-optional` does not download optional dependencies into the store ([#5928](https://github.com/pnpm/pnpm/issues/5928)).

- Node.js runtime resolution now supports Windows ARM64. Node.js 20 and newer resolve native `win-arm64` builds, and older versions fall back to `win-x64` under emulation [#7123](https://github.com/pnpm/pnpm/issues/7123).

- `pnpm install` now runs `node --version` once per run. A workspace whose projects keep their own lockfiles (`sharedWorkspaceLockfile: false`) previously ran the probe once or twice for every project, and on macOS the concurrent launches waited on each other, so a project could wait several seconds before its linking started.

- With `linkWorkspacePackages` enabled, a dependency declared as an `npm:` alias of a workspace project, such as `"math-alias": "npm:math@^1.0.0"`, now counts as a workspace dependency. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it.

- Scripts that `pnpx` and `pnx` run now get pnpm itself as `npm_execpath`. A script that ran `$npm_execpath install` there ran `pnpm dlx install`.

- pnpm now parses the first setting in a `.npmrc` that starts with a UTF-8 byte order mark. Previously, the leading byte order mark caused the first line's key to be ignored [#15353](https://github.com/pnpm/pnpm/issues/15353).

- pnpm now installs a dependency that a package also declares as an optional peer dependency, for example `lightningcss` in some vite builds. The dependency was missing from `node_modules`, so the package failed to import it [#8912](https://github.com/pnpm/pnpm/issues/8912).

- `pnpm outdated` and `pnpm -r outdated` now fail with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when a requested package selector does not match any dependency in the inspected projects [#2319](https://github.com/pnpm/pnpm/issues/2319).

- `pnpm -r outdated --json` now includes every outdated workspace dependency when multiple projects depend on different versions or dependency types of the same package. Such a package is keyed by its current version and dependency type, for example `vue@2.7.14 (dev)` [#7693](https://github.com/pnpm/pnpm/issues/7693).

- `pnpm install` no longer puts a dependency's bin on `PATH` for that dependency's own lifecycle scripts before the bin's file exists. pnpm links such a bin after the dependency's build has run. It also removes such a bin left by an earlier install. This fixes installing the `node` package on Windows [pnpm/pnpm#15501](https://github.com/pnpm/pnpm/issues/15501).

- `pnpm pack` now includes exactly one `package.json` in the archive when the project uses an alternative manifest format. The manifest is included even when `.npmignore` or `files` excludes the source file.

- `node_modules/.package-map.json` no longer contains entries that point at directories that do not exist. Such entries appeared for packages installed only with peer dependencies, most visibly with `enableGlobalVirtualStore` [#14938](https://github.com/pnpm/pnpm/issues/14938).

- Git-hosted dependencies that use a `package.yaml` or `package.json5` manifest now honor its `files` field [#7906](https://github.com/pnpm/pnpm/issues/7906).

- Installing or adding dependencies no longer fails when a previously installed local tarball file was deleted from disk [#8367](https://github.com/pnpm/pnpm/issues/8367).

- The error for an incompatible pnpm-lock.yaml now reports the lockfileVersion the file was generated with and the lockfileVersion the current pnpm supports [#848](https://github.com/pnpm/pnpm/issues/848).

- `.npmrc` and `pnpm-workspace.yaml` files now support npm's `${VAR?}` placeholder. It expands to the value of `VAR`, or to an empty string without a warning when `VAR` is unset [#14404](https://github.com/pnpm/pnpm/issues/14404).

- `pnpm pack` now preserves file executable permissions in the packed tarball when source files are executable on disk.

- Support SemVer build metadata in workspace dependency resolution

- `pnpm patch-commit` now falls back to copying package files when hard linking fails.

- Update lockfile snapshot and prune removed dependencies when `pnpm patch-commit` modifies `package.json` [pnpm/pnpm#6866](https://github.com/pnpm/pnpm/issues/6866).

- `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now work in a project of a workspace with `sharedWorkspaceLockfile: false`. `pnpm patch` failed there with `ERR_PNPM_PATCH_NO_LOCKFILE` after a successful install. The reinstall after committing or removing a patch left the project's own `node_modules` unchanged [#9926](https://github.com/pnpm/pnpm/issues/9926).

- `pnpm install` now applies patches produced by `pnpm patch-commit` when an edit removes the trailing lines of a file along with its newline. The install no longer fails with `ERR_PNPM_INVALID_PATCH` ("expected end of hunk") [#12451](https://github.com/pnpm/pnpm/issues/12451).

- `pnpm peers check` and the install-time peer dependency check now resolve peer dependencies from the workspace root when `resolvePeersFromWorkspaceRoot` is enabled [pnpm/pnpm#14982](https://github.com/pnpm/pnpm/issues/14982).

- `pnpm pipeline` no longer fails when run in a project outside a Git work tree or on a system without `git`. Tasks in those projects run without caching, and pnpm prints a warning explaining why [#15601](https://github.com/pnpm/pnpm/issues/15601).

- Installing through a pnpr server now installs a project's peer dependencies when `autoInstallPeers` is enabled. A project that declared only peer dependencies failed with `ERR_PNPM_OUTDATED_LOCKFILE` or skipped its peers [#14833](https://github.com/pnpm/pnpm/issues/14833).

- pnpm now keeps the configured default registry when `_auth` holds credentials for several registries and some of those registries serve package scopes.

  Lockfile verification checks a tarball hosted on a scoped registry against that registry's metadata, unless the package's own scope has a registry assigned [pnpm/pnpm#15530](https://github.com/pnpm/pnpm/issues/15530).

- `pnpm pack`, `pnpm deploy`, and installs of local directory dependencies now keep symlinks that point to files or directories included in the package. `pnpm pack` leaves out symlinks that point outside the package [#8208](https://github.com/pnpm/pnpm/issues/8208).

- Merging lockfiles now preserves recorded configuration fields such as `overrides`, `neverBuiltDependencies`, `patchedDependencies`, `packageExtensionsChecksum`, `settings`, and `catalogs` [pnpm/pnpm#8366](https://github.com/pnpm/pnpm/issues/8366).

- `pnpm config set` and `pnpm config delete` now preserve comments and repeated keys such as `ca=` in `.npmrc` [pnpm/pnpm#14851](https://github.com/pnpm/pnpm/issues/14851).

- `pnpm version` now applies pending bumps to private workspace packages. A private package's changelog is written to its committed `CHANGELOG.md`, also when `versioning.changelog.storage` is `registry`
  [pnpm/pnpm#13736](https://github.com/pnpm/pnpm/issues/13736),
  [pnpm/pnpm#13519](https://github.com/pnpm/pnpm/issues/13519).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- `pnpm prune --prod` and production installs now prune excluded development dependencies even when lockfile generation is disabled.

- `minimumReleaseAgeExcludePrune` now preserves existing list item indentation when pruning exclude entries in `pnpm-workspace.yaml` [pnpm/pnpm#15571](https://github.com/pnpm/pnpm/issues/15571).

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now work in workspaces with `shared-workspace-lockfile=false`. Once every project has been installed, pnpm drops an entry only if no project lockfile records it. Undecided `allowBuilds` entries are pruned the same way [#14612](https://github.com/pnpm/pnpm/issues/14612).

- `pnpm prune --prod` now removes devDependencies when configured with `lockfile: false` [pnpm/pnpm#2677](https://github.com/pnpm/pnpm/issues/2677).

- `pnpm install` now prunes unreferenced catalog entries from `pnpm-workspace.yaml` when `catalogPrune: true` is configured [#15273](https://github.com/pnpm/pnpm/issues/15273).

- `pnpm publish` now honors `publishConfig["@scope:registry"]` for a package in that scope. It takes precedence over the registry set for the same scope in `.npmrc` and over `publishConfig.registry` [#12071](https://github.com/pnpm/pnpm/issues/12071).

- `pnpm publish` stops waiting after a failed availability probe when the remaining timeout is too short for another polling interval [pnpm/pnpm#15387](https://github.com/pnpm/pnpm/issues/15387).

- Fatal peer dependency errors and their hints are now written to stderr [#5419](https://github.com/pnpm/pnpm/issues/5419).

- Commands that do not use the store no longer create a temporary file in the project directory when they load their settings. These include `pnpm view`, `pnpm config`, `pnpm root`, `pnpm bin`, `pnpm exec`, `pnpm run`, the script shortcuts such as `pnpm test`, and the registry commands such as `pnpm whoami`, `pnpm dist-tag`, and `pnpm search`. `pnpm exec`, `pnpm run`, and the script shortcuts still create one in projects that declare `configDependencies`.

- pnpm no longer writes a package's legacy array-form `engines`, such as `["node >= 0.8"]`, to the lockfile. It was recorded as an object keyed by index, such as `{'0': node >= 0.8}` [#4518](https://github.com/pnpm/pnpm/issues/4518).

- `pnpm install -r` now installs every workspace project when `recursiveInstall` is set to `false` in `pnpm-workspace.yaml` [pnpm/pnpm#7504](https://github.com/pnpm/pnpm/issues/7504).

- `pnpm install` re-resolves a local tarball dependency whose file was replaced at the same path, and installs the new version [#2437](https://github.com/pnpm/pnpm/issues/2437).

- `pnpm install` now relinks a direct dependency whose link in `node_modules` points to a missing target. Before, it reported "Already up to date" and left the broken link [#9758](https://github.com/pnpm/pnpm/issues/9758).

- `pnpm install` now copies the output of a workspace package's own `prepare`, `install`, or `postinstall` script into the injected copies of that package. Before, the injected copies kept only the files that existed before the script ran. `syncInjectedDepsAfterScripts` now also works when `modulesDir` is set [#9464](https://github.com/pnpm/pnpm/issues/9464).

- Removing an entry from `overrides` now re-resolves the packages it targeted. A version the override had locked is no longer kept just because the declared range still accepts it [#4587](https://github.com/pnpm/pnpm/issues/4587).

- `pnpm install` now re-resolves a dependency when its manifest range is updated from a prerelease to a stable version. The lockfile previously retained the prerelease version and caused `--frozen-lockfile` to fail [pnpm/pnpm#15528](https://github.com/pnpm/pnpm/issues/15528).

- `pnpm patch-commit` now resolves default patch directory locations when passed a package name or package specifier (such as `pnpm patch-commit <pkg>` or `pnpm patch-commit <pkg>@<version>`).

- `pnpm restart` now runs the "stop" and "start" scripts when the package has no "restart" script. Previously it ran "stop" and then failed with "Missing script: restart" [#4750](https://github.com/pnpm/pnpm/issues/4750).

- On Windows, `pnpm clean` and installs no longer fail immediately when another process uses a package in `node_modules`. pnpm waits up to a minute for an open file. It waits up to 5 seconds for a running program [pnpm/pnpm#15081](https://github.com/pnpm/pnpm/issues/15081).

- `pnpm install` reuses a package already present in the store when an existing lockfile entry satisfies the dependency, avoiding registry requests that fail without authorization [pnpm/pnpm#2522](https://github.com/pnpm/pnpm/issues/2522).

- `pnpm run` and `pnpm exec` now print `No projects matched the filters in "<workspace>"` when `--filter` selects no project [#8408](https://github.com/pnpm/pnpm/issues/8408).

- `pnpm run` with `--loglevel` set to `warn`, `error`, or `silent` (or the same `loglevel` setting) no longer prints the `$ <command>` line before a script, nor the summary of the install that `verifyDepsBeforeRun` runs first. Both are info-level output [#8944](https://github.com/pnpm/pnpm/issues/8944).

- A `runtime:` version range that contains `||` or a space, such as a `devEngines.runtime` version of `^22.18.0 || ^24.0.0`, now installs the requested runtime. pnpm used to install the npm package with the same name, such as `node` [#14817](https://github.com/pnpm/pnpm/issues/14817).

- A script that pnpm runs without a terminal now ends when pnpm itself is killed. Killing pnpm's process group, as Playwright's `webServer` does to stop the command it started, used to leave the script running and holding the caller's output pipes open [#15555](https://github.com/pnpm/pnpm/issues/15555).

- Fixed scripts failing with errors such as `'an-compile' is not recognized` when `scriptShell` is set to `cmd.exe` on Windows [#7181](https://github.com/pnpm/pnpm/issues/7181).

- A script killed by a signal now fails with an error that names the signal, such as `Command failed with signal SIGKILL.` [#9821](https://github.com/pnpm/pnpm/issues/9821).

- Secondary dependencies now prefer the version resolved by the local project's direct dependencies over versions from sibling workspace projects [pnpm/pnpm#7191](https://github.com/pnpm/pnpm/issues/7191).

- `pnpm self-update` no longer leaves the previous pnpm in the global packages when it was installed as `@pnpm/exe`. `pnpm ls -g` now lists a single pnpm [#14709](https://github.com/pnpm/pnpm/issues/14709).

- `pnpm setup` now installs pnpm under the package name `pnpm`, the name `pnpm self-update` uses, so both leave the same shims in the global bin directory. After a self-update on Windows, PowerShell ran pnpm through `pnpm.cmd` and asked "Terminate batch job (Y/N)?" on Ctrl+C [#15567](https://github.com/pnpm/pnpm/issues/15567).

- `pnpm setup` no longer deletes aliases and other lines that sit between a `# pnpm` comment and the pnpm block in a shell startup file [#7067](https://github.com/pnpm/pnpm/issues/7067).

- `pnpm setup` failed with `Text file busy (os error 26)` when `$PNPM_HOME/bin` already held `pn`, `pnpx`, or `pnx` as links to the running pnpm executable. It now replaces those files and completes [#15494](https://github.com/pnpm/pnpm/issues/15494).

- On Windows, bin shims run from Git Bash, MSYS2, or Cygwin now pass `NODE_PATH` to Node.js as Windows paths. A project installed from cmd or PowerShell gave its bins a `NODE_PATH` under the Git install directory when they ran from Git Bash. Installing again replaces the shims already in `node_modules` [#3360](https://github.com/pnpm/pnpm/issues/3360).

- `pnpm install` no longer adds `allowBuilds` placeholder entries to `pnpm-workspace.yaml` when it runs in CI or without a terminal. Interactive installs still add them [#11574](https://github.com/pnpm/pnpm/issues/11574).

- pnpm no longer treats manifests inside its store, cache, state, or modules directories as workspace projects. Before, a `storeDir` inside the workspace could let lifecycle scripts of packages in the store run without `allowBuilds` approval [#15033](https://github.com/pnpm/pnpm/issues/15033).

- Executable linking now checks for all executable permission bits before skipping chmod. Targets with partial execute permissions previously skipped permission repair and remained non-executable for other users [pnpm/pnpm#3699](https://github.com/pnpm/pnpm/issues/3699).

- Do not run `prepare` lifecycle scripts when `pnpm install` is invoked with options that omit devDependencies (e.g., `pnpm install --prod`) or with package arguments.

- `pnpm pack` and `pnpm publish` now include bundled dependencies when using the isolated linker. This covers workspace packages and the dependencies of each bundled package. Bundled dependencies are also included when `publishConfig.directory` selects a build directory [pnpm/pnpm#1643](https://github.com/pnpm/pnpm/issues/1643).

- `pnpm store status` no longer falsely reports packages with build or postinstall scripts as modified in the store. Packages requiring builds have their integrity verified against recorded side effects, or are recognized as built packages.

- `pnpm store status` now lists only the modified packages when packages in the store were mutated. It previously also suggested running `pnpm install --force` to refetch them [#919](https://github.com/pnpm/pnpm/issues/919).

- `pnpm store status` no longer reports packages with peer dependencies or skipped optional dependencies as modified [#15383](https://github.com/pnpm/pnpm/issues/15383).

- Tarball URLs recorded in the lockfile now strip default HTTP and HTTPS ports (`:80` and `:443`) [pnpm/pnpm#15539](https://github.com/pnpm/pnpm/issues/15539).

- On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).

- When the registry stops sending data for longer than `fetchTimeout`, pnpm now reports that the metadata or tarball request timed out. Previously the error did not mention the timeout [#3646](https://github.com/pnpm/pnpm/issues/3646).

- `pnpm install` now fetches committed submodules of git dependencies [pnpm/pnpm#1470](https://github.com/pnpm/pnpm/issues/1470).

- Follow symlinked directories when discovering workspace packages matching `packages` globs.

- Fixed `pnpm install` for workspace projects reached through a symlink, such as a `packages` directory that links to a folder outside the workspace. pnpm now installs their dependencies, and the links in their `node_modules` resolve [#1044](https://github.com/pnpm/pnpm/issues/1044).

- `packageExtensions` ranged selectors (such as `@<X` or `@*`) no longer match dependencies that ship without a `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).

- pnpm no longer hangs for up to 5 minutes after a pnpm process was killed while setting up the pnpm version pinned in `packageManager` or `devEngines` [#15360](https://github.com/pnpm/pnpm/issues/15360), [#15393](https://github.com/pnpm/pnpm/issues/15393). The killed process left behind a lock that every later pnpm command in the project waited on. pnpm now detects that the process holding a lock is gone and takes the lock over at once. The same applies to the locks pnpm takes while installing a managed runtime or writing the global bin directory. Two pnpm processes that are both still running keep waiting for each other as before.

- `pnpm install --prod` and other installs that skip `devDependencies` no longer run the `pnpm:devPreinstall` script [#7065](https://github.com/pnpm/pnpm/issues/7065).

- Bin linking leaves workspace and linked dependency files outside node_modules unchanged. Already executable bin files no longer receive redundant permission changes.

- Trim leading and trailing whitespace from dependency override selectors in `pnpm.overrides` [#6356](https://github.com/pnpm/pnpm/issues/6356).

- With `trustPolicy: no-downgrade`, pnpm now resolves the newest matching version that is not a trust downgrade. Previously a dependency failed with `ERR_PNPM_TRUST_DOWNGRADE` even when an older version satisfied its range. `pnpm self-update` picks its target version the same way. A request for an exact version still fails [#14176](https://github.com/pnpm/pnpm/issues/14176).

- Expand environment variables in `_auth.authToken` values loaded from global `config.yaml` and `pnpm_config__auth`.

- `packageExtensions` and `overrides` entries with a ranged selector (such as `@<X` or `@*`) no longer match a local directory dependency that has no `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).

- `pnpm unlink` now removes the `link:` dependency that `pnpm link <dir>` added to `package.json`. The linked package is removed from `node_modules` and the lockfile. A `link:` dependency to another directory is kept [#4219](https://github.com/pnpm/pnpm/issues/4219).

- `pnpm install` now updates an injected workspace dependency after that package's own dependencies change, when `shared-workspace-lockfile` is `false` [#7209](https://github.com/pnpm/pnpm/issues/7209).

- `pnpm update` now keeps a version range whose shape has no save prefix, such as `<= 3.0.0` or `>=1.0.0 <2.0.0`, when the updated version still satisfies it. Before, `<= 3.0.0` became `^3.0.0` [#6714](https://github.com/pnpm/pnpm/issues/6714).

- `pnpm update <pkg>` now moves a package off a locked version the registry no longer serves, such as an unpublished release. The lockfile check for supply-chain policies such as `minimumReleaseAge` used to reject that version before the update could replace it [pnpm/pnpm#9953](https://github.com/pnpm/pnpm/issues/9953).

- `pnpm update --prod` no longer installs devDependencies when run in a project installed with `--prod` [#8038](https://github.com/pnpm/pnpm/issues/8038).

- Concurrent `pnpm run` and `pnpm exec` commands now serialize their dependency installs [#14551](https://github.com/pnpm/pnpm/issues/14551).

- `verifyDepsBeforeRun` checks now account for project-specific `packageConfigs` overrides in workspaces with `sharedWorkspaceLockfile: false` [pnpm/pnpm#15545](https://github.com/pnpm/pnpm/issues/15545).

- pnpm now escapes trailing dots and spaces in `node_modules/.pnpm` directory names. Windows strips these characters, so a dependency such as `"parent-pkg": "file:../"` created a directory that could not be deleted or failed to install [#8101](https://github.com/pnpm/pnpm/issues/8101).

- `pnpm add` now warns when replacing an existing dependency with a specifier pointing to a different source [#14869](https://github.com/pnpm/pnpm/issues/14869).

- `pnpm link` now warns when linking a package that declares one or more peer dependencies, explaining that the linked dependency will not resolve peer dependencies from the target `node_modules` and suggesting the `file:` protocol instead.

- pnpm now warns when a workspace install covers a project that has its own `pnpm-workspace.yaml`. The nested file's settings, such as `patchedDependencies`, do not apply when the outer workspace installs that project. pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace root [#11724](https://github.com/pnpm/pnpm/issues/11724).

- pnpm now prints a warning when a `.npmrc`, `auth.ini`, or the file set by `npmrcAuthFile` exists but cannot be read. The settings in such a file were ignored without any message. A `.npmrc` that contains invalid UTF-8 is now read [#5065](https://github.com/pnpm/pnpm/issues/5065).

- A lockfile entry whose resolution is unchanged now keeps its recorded `deprecated` message [#5772](https://github.com/pnpm/pnpm/issues/5772).

- Exclude entries written to `pnpm-workspace.yaml` now match the dominant quote style of the file [pnpm/pnpm#15079](https://github.com/pnpm/pnpm/issues/15079).

- A `workspace:` dependency now resolves to a workspace project whose version is not valid semver, such as `1` or `1.0`. `workspace:*`, `workspace:^`, and `workspace:~` match it. A range identical to the version also matches it [#4567](https://github.com/pnpm/pnpm/issues/4567).

- `pnpm install` now re-resolves a workspace project's auto-installed peer dependency when another workspace project changes its specifier for that package to one that excludes the locked version but still overlaps the peer range. The peer then resolves to the version a fresh install would pick [#11800](https://github.com/pnpm/pnpm/issues/11800).

- `pnpm publish` and `pnpm pack` now report a missing `version` or `name` field on a workspace dependency. Previously, pnpm reported that the dependency was not installed [#4164](https://github.com/pnpm/pnpm/issues/4164).

- `pnpm install` no longer fails when writing the workspace state file encounters an error. Failures to update the state file now emit a warning instead of aborting the install [pnpm/pnpm#14550](https://github.com/pnpm/pnpm/issues/14550).

- pnpm no longer expands environment variables in a `userAgent` set in a project's `pnpm-workspace.yaml`. A `userAgent` with a placeholder in that file is now ignored. Before this fix, pnpm sent the variable's value to the configured registry [#15415](https://github.com/pnpm/pnpm/issues/15415).
