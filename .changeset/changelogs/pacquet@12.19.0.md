## 12.19.0

pnpm 12.19.0 adds `pnpm fund`, lets the `loaded` node linker handle packages that need a build, and adds the `projectScopedOptionalPeers` setting and an experimental JSON lockfile format. It also includes a security fix: an offline install error no longer prints registry credentials. pnpm now checks the `engines` and platform fields of every workspace project, and many hoisted-linker and frozen-lockfile bugs are fixed.

### Minor Changes

- Added `pnpm fund`, which lists the funding URLs declared by the installed dependencies in the same tree `npm fund` prints. Use `--json` for the `npm fund --json` report and `--recursive` or `--filter` to report on workspace projects. `pnpm fund <pkg>` opens the funding URL of an installed package, and `--which` picks one when it lists several [#7354](https://github.com/pnpm/pnpm/issues/7354).

- With `nodeLinker.type: loaded`, packages that need a build no longer have to be listed in `nodeLinker.excluded`. A package with a matching side-effects cache entry loads from the store. If side-effects cache reads and writes are enabled, pnpm builds an approved package once, saves the result to the side-effects cache, and then loads it from the store. This includes a build that changes no files in its package, so its script does not run again in other projects that use the same store. List a package in `sideEffectsCache.exclude` if its build must run in every project, such as a git hook installer. pnpm materializes other cache misses, packages with native `.node` addons, and their dependencies automatically.

  pnpm also scans JavaScript sources for filesystem imports and for `__dirname` or `import.meta`, and materializes matching packages. The scan does not detect computed imports. List such packages in `nodeLinker.excluded`.

- Added the `projectScopedOptionalPeers` setting. When it is `true`, a project gets a missing optional peer dependency only if the project's own dependencies install a version of it. Another workspace project's dependencies, such as its `devDependencies`, no longer supply one. `dedupePeerDependents` keeps deduplicating everything else [#16666](https://github.com/orgs/pnpm/discussions/16666).

- Added the experimental `lockfile.format: v13` setting. With it, pnpm writes `pnpm-lock.yaml` as JSON, indented with tabs, with a trailing comma after every entry. The file has one `packages` entry per package version in place of the `packages` and `snapshots` sections, and each entry records the versions of its peer dependencies as its properties. Updating a peer dependency then changes only the lines that record its version. pnpm reads lockfiles in both formats whatever the setting is.

- `pnpm store add` now accepts directories of tarballs, such as `pnpm store add ./tarballs`, and adds every `.tgz`, `.tar.gz`, and `.tar` file inside them recursively. `pnpm store add` is now faster when it adds many packages or local tarballs [#17875](https://github.com/pnpm/pnpm/issues/17875).

- With `supportedArchitectures` set, `pnpm install` and `pnpm fetch` also store, without linking them, the dependencies of the archives kept for the other listed platforms. A platform added to the setting gets its archives and their dependencies on the next install. Optional dependencies a listed platform cannot run are not stored, and one that fails to download is skipped. Dependencies from git repositories are not stored for other platforms.

- When `supportedArchitectures` names exactly one platform and not `current`, pnpm links that platform's variant of a package published with platform variants, together with that variant's dependencies. This holds for the list form and for the `os`, `cpu`, and `libc` form. A variant whose floor or ABI the listed platform does not state is not linked for it: pnpm links another variant that fits, or the version's own archive. Changing the setting, or the platform of the host, such as a `node_modules` mounted into a musl container or a Node.js with another ABI, relinks the package on the next install, even from a frozen lockfile.

- pnpm now provides a prebuilt binary for NetBSD 10 on x64 [#17803](https://github.com/pnpm/pnpm/issues/17803).

### Patch Changes

#### Security

- The error for a tarball that is missing from the store in offline mode no longer prints the credentials or token in the tarball URL.

#### Installs that failed or hung

- `pnpm install`, `pnpm ci`, and `pnpm install-test` outside a workspace now fail with `ERR_PNPM_NO_PKG_MANIFEST` when the directory has no `package.json`, `package.json5`, or `package.yaml`. They no longer create a `package.json`, a lockfile, and `node_modules` there [#17727](https://github.com/pnpm/pnpm/issues/17727).

- Fixed `pnpm install` hanging when a concurrent tarball download or manifest read is cancelled while other tasks are waiting on the same archive.

- Running a global command such as `node` in a project that pins its version no longer hangs while pnpm installs that version. The hang kept starting new processes until the machine ran out of memory.

- A global command such as `node` now runs the version a project pins when `PNPM_CONFIG_LOCKFILE_DIR`, `PNPM_CONFIG_VIRTUAL_STORE_ONLY`, or `PNPM_CONFIG_SYMLINK` is set in the environment. Before, the dispatch failed with "the installed node executable is not under" the store.

- `pnpm install` now fetches fresh metadata for a package when a version allowed by `minimumReleaseAgeExclude` is missing from the cached metadata. Before, it could fail with `ERR_PNPM_NO_MATCHING_VERSION` without contacting the registry [#17723](https://github.com/pnpm/pnpm/issues/17723).

- A `${VAR}` placeholder in a registry URL in `pnpm-workspace.yaml` or the project `.npmrc` now fails the command. pnpm does not expand environment variables in these files. Before, pnpm ignored such a URL and resolved packages from another registry, such as npmjs.org. pnpm now warns when it ignores a placeholder in a proxy, `pnprServer`, or `userAgent` setting in `pnpm-workspace.yaml` [#17762](https://github.com/pnpm/pnpm/issues/17762).

- Fixed local package installs failing while a workspace lockfile is being written.

- `pnpm self-update` and switching to the `packageManager` version now work behind a proxy that ends a response by closing the TLS connection without a `close_notify` alert. The registry signature check failed there with `ERR_PNPM_PNPM_ENGINE_IDENTITY_UNVERIFIABLE` [#17763](https://github.com/pnpm/pnpm/issues/17763).

- A package tarball that keeps failing to download now fails the install after `fetchRetries` retries. pnpm used to download it again with a second full set of retries, which doubled the wait. The error now names the actual download failure.

- When the native binary of the pnpm version a project pins fails to download, pnpm now reports the download error. It used to report only that no native binary was found for the host.

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build on macOS with the sandbox enabled. Installing again replaces the shims already in `node_modules` [#17750](https://github.com/pnpm/pnpm/issues/17750).

- Fixed pnpm leaving process pipes open after it exits on macOS [pnpm/tasks#229](https://github.com/pnpm/tasks/issues/229).

#### Engines and platform checks

- `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm remove` now check the `engines`, `os`, `cpu`, and `libc` fields of every workspace project, including the workspace root. `pnpm run -r` and `pnpm exec -r` check them before running anything. An incompatible `engines.pnpm` fails the command. An incompatible `engines.node` prints a warning, or fails the command with `engineStrict` enabled. An unsupported platform prints a warning. A project that downloads its own Node.js through `devEngines.runtime` is checked against that Node.js [#17751](https://github.com/pnpm/pnpm/issues/17751) [pnpm/tasks#281](https://github.com/pnpm/tasks/issues/281) [pnpm/tasks#299](https://github.com/pnpm/tasks/issues/299).

- Fixed missing warnings when a required dependency does not support the current Node.js version.

- An optional dependency that declares no `os`, `cpu`, or `libc` field is no longer skipped because its name contains only an operating system word, such as `get-windows` on macOS. pnpm now infers its platform from the name only when the name also has a cpu or libc token, such as `-win32-x64` [#17774](https://github.com/pnpm/pnpm/issues/17774).

#### Peer dependencies

- With `dedupePeerDependents: false`, a missing optional peer is no longer resolved from a version that only another workspace project depends on. The workspace root's dependencies still count if `resolvePeersFromWorkspaceRoot` is `true`. Before, a tool in one project's `devDependencies`, such as `webpack-cli`, could become an optional peer of another project's production dependencies [#16666](https://github.com/orgs/pnpm/discussions/16666).

- `pnpm install` no longer gives a project an optional peer dependency that it skipped because a required peer would conflict with the project's dependencies. Before, `dedupePeerDependents` merged the project's copy of the package that declares the peer with another project's copy that resolves it [#17781](https://github.com/pnpm/pnpm/issues/17781).

- `pnpm install` no longer fills in a missing optional peer dependency if a required peer it brings in conflicts with a version the project depends on. Before, such a peer was installed along with it and reported as unmet [#17718](https://github.com/orgs/pnpm/discussions/17718).

- `pnpm install --filter` in a sparse checkout now counts the dependencies of workspace projects that are missing from disk when it resolves peer dependencies and marks packages as optional. The lockfile it writes no longer differs from a full checkout in those entries [#17791](https://github.com/pnpm/pnpm/issues/17791).

#### Frozen lockfile and workspace links

- `pnpm install --frozen-lockfile` now succeeds when a workspace project recorded in `pnpm-lock.yaml` has an empty directory, such as a git submodule that was not checked out [#17712](https://github.com/pnpm/pnpm/issues/17712).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a workspace package linked through a plain semver range, such as `^7.9.5`, has changed to a version outside that range [#17806](https://github.com/pnpm/pnpm/issues/17806).

- `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` now accept a `workspace:` dependency on a project whose version is not valid semver, such as `workspace:1` on a project at version `1`. Before, they reported the lockfile that `pnpm install` had just written as out of date.

- `pnpm install --frozen-lockfile` now accepts a lockfile when an injected workspace package declares a `file:` or `link:` dependency with backslashes, such as `file:..\vendor`, or with a slash after the protocol, such as `file:/./vendor`. Before, the install reported the lockfile as out of date right after `pnpm install` had written it [#17720](https://github.com/pnpm/pnpm/pull/17720).

- When a package injected through a path, such as `file:../lib` or `../lib`, gives one of its dependencies a `catalog:` specifier, `pnpm install --frozen-lockfile` now reports the lockfile as outdated. For a bare path such as `../lib`, `pnpm install` now also fails the way a fresh resolution does. Before, these installs accepted the existing lockfile [#17722](https://github.com/pnpm/pnpm/pull/17722).

- Dependencies declared as `workspace:` entries in catalogs now remain in the lockfile when `excludeLinksFromLockfile` is enabled. Frozen installs can restore these links after `node_modules` is removed [pnpm/tasks#260](https://github.com/pnpm/tasks/issues/260).

- With `dedupeDirectDeps`, `pnpm install` now compares `link:` dependencies by the directory they point at. A project's `link:` dependency is removed when the workspace root links the same directory under a different path. It is kept when the root's `link:` dependency has the same text but points at another directory. pnpm reports a dependency the root provides as removed only once, when it removes the project's link. The summary shows the package version of a removed `link:` dependency [pnpm/tasks#293](https://github.com/pnpm/tasks/issues/293).

#### Hoisted node linker and custom modulesDir

- With `nodeLinker: hoisted`, a package that runs a lifecycle script or has a patch is now cloned or copied into `node_modules`, as with the isolated linker. A build that rewrites one of its files no longer changes the store copy shared with other projects.

- With `nodeLinker: hoisted`, pnpm no longer deletes a directory outside `node_modules` when it removes a package whose scope directory is a symlink.

- With `nodeLinker: hoisted`, `pnpm install --filter` in a sparse checkout now drops previously installed workspace projects that are missing from disk from the hoisted `node_modules` tree. It no longer recreates `node_modules` inside a missing project's directory. Packages that only the missing project used are also removed from the root `node_modules`.

- With `nodeLinker: hoisted` and `dedupeDirectDeps`, `pnpm install` now removes a project's `link:` dependency that the workspace root provides, and reports each `link:` dependency it removes. Removing the last scoped `link:` dependency of a scope also removes the empty `@scope` directory.

- With `nodeLinker: hoisted` and a custom `modulesDir`, pnpm now installs a workspace project's dependencies that cannot be hoisted to the root into the project's `modulesDir`. The next install that relinks the project removes the copies that earlier installs left in its `node_modules`. A later install restores the project's `modulesDir` when it was removed. Package directories that pnpm did not install are now also moved to `.ignored` in a custom `modulesDir`.

- Fixed installs in a workspace whose `modulesDir` is an absolute path, including one that goes through a symlink such as `/tmp` on macOS, or a path that starts with `..`. Dependencies of workspace projects no longer end up outside the workspace, and the root project's dependencies, `link:` dependencies, and commands are linked into the configured directory. `pnpm bin`, `pnpm root`, `pnpm run`, `pnpm exec`, and `pnpm version` now use that directory too [pnpm/tasks#285](https://github.com/pnpm/tasks/issues/285).

#### Store and builds

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Packages that an older pnpm version added to the store now run their install scripts. Before, pnpm could skip those scripts and hardlink the package's files to the store.

- `pnpm install` now keeps patches to bundled dependencies when it restores a package from the side-effects cache, including files a patch adds [#17734](https://github.com/pnpm/pnpm/issues/17734).

- Local installs on macOS now keep cached clones separate from the global virtual store. `pnpm store prune` removes this cache, so unused cached packages no longer accumulate [#17907](https://github.com/pnpm/pnpm/issues/17907).

- `pnpm store prune` now stops if a dependency link cannot be read. This keeps installed packages from being removed after an incomplete scan.

- A repeat `pnpm install` now runs a full install when the `runtimeOnFail` setting changes. Before, it printed "Already up to date" and did not install the runtime a project declares in `devEngines.runtime` [pnpm/tasks#303](https://github.com/pnpm/tasks/issues/303).

- The sync of an injected workspace dependency now copies a file whose source has reached the filesystem's limit on hard links. `pnpm patch-commit` now copies a file into its scratch directory when the filesystem refuses to hard link it, unless the path is missing or already occupied [pnpm/tasks#283](https://github.com/pnpm/tasks/issues/283).

#### Speed

- `pnpm install` is faster when it clones files from the store, which it does by default on macOS. On a Mac with endpoint security software, a cold install of a project with about 1,350 packages took 29% less time [pnpm/tasks#280](https://github.com/pnpm/tasks/issues/280).

- `pnpm install` unpacks packages that contain many files faster. Unpacking `typescript` took 30% less time in our measurements [pnpm/tasks#288](https://github.com/pnpm/tasks/issues/288).

- Resolving against a registry that sends `Cache-Control: no-cache`, such as Azure Artifacts, is fast again when the lockfile already decides the version. pnpm reads the cached metadata for a pinned version, or for a range the lockfile version is proven to win, without a registry request [#17758](https://github.com/pnpm/pnpm/issues/17758).

- POSIX bin shims start fewer processes. Running a dependency's bin through its shim from Git Bash on Windows starts 7 processes where it started 15. Installing again replaces the shims already in `node_modules`.

#### Windows

- pnpm now installs dependencies and runs lifecycle scripts from projects on mapped Windows network drives. When a network share refuses the links in `node_modules`, the error now suggests setting `nodeLinker: hoisted` or moving the project to a local drive [#17747](https://github.com/pnpm/pnpm/issues/17747).

- `packageImportMethod: auto` now tries hardlinking before cloning, as it does on Linux. Installs from a warm store on a Dev Drive are faster. An install no longer fails when the store refuses a hardlink with an access-denied error, and pnpm tries cloning or copying the file next [#17809](https://github.com/pnpm/pnpm/issues/17809).

- `pnpm run` and `pnpm setup` no longer clear the visible terminal when a project path contains non-ASCII characters.

- A failed attempt to create a directory junction in `node_modules` no longer leaves a `.pnpm-junction-*` directory behind.

- `pnpm` starts faster. It loads system libraries such as `user32.dll` and `shell32.dll` the first time a command uses them, so `pnpm --version` loads 11 libraries where it loaded 25.

#### pnpm deploy

- `pnpm deploy` now runs the build scripts of a workspace project that it installs as a `file:` dependency, without a machine-specific `allowBuilds` key. An `allowBuilds` entry that denies the project still applies [#17740](https://github.com/pnpm/pnpm/issues/17740).

- `pnpm deploy` no longer fails when the deployed project depends on a workspace package that sets `publishConfig.directory`. pnpm installs that package from its publish directory and runs its build scripts [#17771](https://github.com/pnpm/pnpm/pull/17771).

- `pnpm deploy` without a shared workspace lockfile now resolves a relative `workspace:` path, such as `workspace:../lib`, from the source project's directory. It used to be resolved from the deploy target and fail [#17804](https://github.com/pnpm/pnpm/issues/17804).

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [#17888](https://github.com/pnpm/pnpm/issues/17888).

#### Other commands and settings

- `pnpm dist-tag` and `pnpm unpublish` now offer browser-based authentication when the account's two-factor authentication uses a security key. Before, they asked for a one-time password in that case [#17832](https://github.com/pnpm/pnpm/issues/17832).

- `pnpm publish` with provenance now downloads Sigstore's current signing configuration before signing. A failed or slow download is retried with the same retry policy and deadline as the signing requests.

- `pnpm pack` and `pnpm publish` now apply the first rule of a `.npmignore` or `.gitignore` that starts with a UTF-8 BOM.

- `pnpm outdated` now checks dependencies when the project has no lockfile, for example with `lockfile: false`. The `Current` column shows the installed version, or the lowest version allowed by the declared range if the dependency is not installed [#17811](https://github.com/pnpm/pnpm/issues/17811).

- `pnpm self-update` now updates the project's `engines.pnpm` range along with its pnpm pin, keeping the `^` or `~` operator [#17732](https://github.com/pnpm/pnpm/issues/17732).

- On Linux and macOS, `pnpm run` no longer sometimes reports a lifecycle failure after `Ctrl+C` interrupts a script.

- Config dependencies use the declared registry `serverType` when recording and installing packages [pnpm/tasks#236](https://github.com/pnpm/tasks/issues/236).

- Relative directory selectors set on `config.filter` or `config.filterProd` by an `updateConfig` hook now resolve from the workspace root even when `--dir` is passed or `pnpm` runs from a subdirectory [#17906](https://github.com/pnpm/pnpm/issues/17906).

- Commands that add settings to `pnpm-workspace.yaml` now fail with an error if the file contains only `null`, and leave the file unchanged. Before, they wrote invalid YAML.

- Shell tab-completion now completes directory and file paths for path options such as `-C`, `--dir`, `--prefix`, `--store-dir`, and `--env-file`. Before, options without a fixed list of values offered command names [#17884](https://github.com/pnpm/pnpm/issues/17884).
