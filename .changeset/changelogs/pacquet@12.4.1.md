## 12.4.1

### Patch Changes

- `pnpm install` now copies a package file whose store entry has reached the filesystem's limit on hard links to one file. NTFS allows 1024 names per file and ext4 allows 65000. Such a file failed the install under `packageImportMethod: hardlink`. Under `auto` it stopped pnpm hard linking for the rest of the install.

- `pnpm install` no longer writes a package file through a symlink left at the path it is importing to. Copying such a file overwrote whatever the link pointed at. When the link pointed nowhere, the copy created that file. An executable package file also made the link's target executable.

- `pnpm dedupe` now preserves compatible auto-installed peers when another workspace project depends on a newer major. Repeated runs previously alternated between compatible and incompatible peer versions [pnpm/pnpm#14697](https://github.com/pnpm/pnpm/issues/14697).

- `pnpm audit --fix` and the `minimumReleaseAgeStrict` approval prompt no longer drop the comments of `minimumReleaseAgeExclude` when they append an entry to the list in `pnpm-workspace.yaml`. The rest of the list is now left as written.

  The `trustPolicyExcludePrune` and `minimumReleaseAgeExcludePrune` cleanups leave the comments of the entries they keep in place, too.

- Fixed `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` mutating global bins or install directories after only partially reading an installed package group. If any declared package manifest is missing, malformed, or unreadable, pnpm now fails before activation or removal and leaves the existing global installation intact [pnpm/pnpm#13796](https://github.com/pnpm/pnpm/issues/13796).

- Fixed registry requests crashing on Android. pnpm now uses bundled CA certificates on Android [pnpm/pnpm#14777](https://github.com/pnpm/pnpm/issues/14777).

- `pnpm peers check` no longer reports a peer dependency declared as `workspace:^`, `workspace:~`, or a bare `workspace:` as unmet. pnpm reported these as unmet whatever version the linked workspace project supplied [#14770](https://github.com/pnpm/pnpm/issues/14770).

- `pnpm install` no longer fails with "Invalid cross-device link" while preserving a package's nested `node_modules` directory during a Docker build [#14758](https://github.com/pnpm/pnpm/issues/14758).

- `pnpm dedupe` now processes all workspace projects by default, including workspaces with a separate lockfile per project. Workspace filters now select which projects to process. `--fail-if-no-match` now exits with an error when no projects match [pnpm/pnpm#14732](https://github.com/pnpm/pnpm/issues/14732).

- `pnpm install`, `pnpm add`, and `pnpm dedupe` now apply `ignoredOptionalDependencies`. Matching optional dependencies are left out of the lockfile and are not installed. pnpm 12 installed them whenever it resolved dependencies from scratch [pnpm/pnpm#14729](https://github.com/pnpm/pnpm/issues/14729).

- Fixed dangling dependency links in workspace packages located above the workspace root [pnpm/pnpm#14726](https://github.com/pnpm/pnpm/issues/14726).

- `pnpm --filter` directory selectors now support `?` wildcards and character classes such as `[ab]`. A `*` or `?` wildcard no longer selects a directory whose name starts with a dot, matching pnpm 11.

- Fixed `pnpm install` and `pnpm dlx` failing with "Permission denied" on Android when the filesystem denies hardlinks or reflinks. Automatic package imports now fall back to copying on `EACCES` [pnpm/pnpm#14780](https://github.com/pnpm/pnpm/issues/14780).

- `pnpm install` no longer links transitive dependencies with plain version ranges to workspace packages when `linkWorkspacePackages` is `true`, even with `preferWorkspacePackages` enabled. Use `linkWorkspacePackages: deep` to enable these links. Fixes [pnpm/pnpm#14781](https://github.com/pnpm/pnpm/issues/14781).

- Installing several packages from the same Git repository and commit now downloads the source once per install. Each package still runs its prepare scripts in its own copy of the checkout [pnpm/pnpm#14725](https://github.com/pnpm/pnpm/issues/14725).

- Under `nodeLinker: hoisted`, `pnpm install` no longer re-imports packages that are already in place. A repeat install used to replace the whole `node_modules` tree and report `Packages: +N`. A package is still imported when its directory is missing, when its `package.json` no longer carries the installed version, when it is a `file:` dependency, and when it is patched. Lifecycle scripts no longer run again for a package left in place, but `pnpm rebuild` and a change to `allowBuilds` still reach it.

- `pnpm install` no longer fails with `Operation not permitted` when the filesystem refuses a hardlink or a reflink. Under `packageImportMethod: auto` and `clone-or-copy`, pnpm copies the file. EdenFS checkouts, which have no hardlinks, and rootless containers, which refuse the clone syscall, both hit this. An explicit `packageImportMethod: hardlink` or `clone` still reports the error [#14722](https://github.com/pnpm/pnpm/issues/14722).

- `pnpm install` no longer fails on a package tarball that carries a file at the archive root, such as the `._*` entries macOS `tar` adds. The file is installed at the root of the package [#14701](https://github.com/pnpm/pnpm/issues/14701).

  A `file:` tarball packed without the usual `package/` directory is now recorded under the name and version from its own `package.json`. It was recorded under the alias the dependency was given, at version 0.0.0.

- `pnpm install` now checks the store's files only for the packages it links into `node_modules`. A warm restore of a workspace using the global virtual store no longer stats every file of every package in the lockfile before skipping the slots that already exist [#14540](https://github.com/pnpm/pnpm/issues/14540).

- The `pnpm --help` description no longer labels the package manager as experimental.

- `pnpm deploy --legacy` now prefers dependency versions pinned in the source workspace lockfile when they still satisfy the deployed project's range [pnpm/pnpm#13857](https://github.com/pnpm/pnpm/issues/13857).

- pnpm no longer creates a project `pnpm-lock.yaml` when `devEngines.packageManager.onFail` is `download` and lockfile writing is turned off with `lockfile: false` or `--no-lockfile`. pnpm still switches to the pinned version [#14728](https://github.com/pnpm/pnpm/issues/14728).

- Reduced memory allocations when pnpm verifies large lockfiles [#14706](https://github.com/pnpm/pnpm/issues/14706).

- `NO_PROXY` entries that start with a dot, such as `.npmjs.org`, now bypass the proxy for the domain and its subdomains [#14686](https://github.com/pnpm/pnpm/issues/14686).

- `pnpm config set --global node-download-mirrors` no longer rejects the key. The global config file already accepted `nodeDownloadMirrors`, but the command refused to write it [#13611](https://github.com/pnpm/pnpm/issues/13611).

- Fixed intermittent access-denied errors when concurrent tasks save cache state on Windows.

- `pnpm run "/pattern/" --no-bail` now lets every matched script finish after one of them fails. The command then exits with `ERR_PNPM_RUN_FAILED`. Its message lists the scripts that failed, in the order they were selected [#14718](https://github.com/pnpm/pnpm/issues/14718).

- `pnpm pipeline` no longer fails on a project that tracks a symlink, such as a `CLAUDE.md` pointing at `AGENTS.md`. Changing a symlinked input's target invalidates that task's cache. `pnpm pipeline --no-cache` no longer hashes task inputs [#14692](https://github.com/pnpm/pnpm/issues/14692).

- `pnpm install` and `pnpm add` no longer leave a dangling symlink in `node_modules` when a project starts depending directly on a package that the lockfile holds only as a transitive dependency with resolved peer dependencies [#14714](https://github.com/pnpm/pnpm/issues/14714).

- Sped up `pnpm install` in Cargo workspaces with many member crates. Repeated installs reuse verified Cargo checksum metadata.

- `pnpm install` and `pnpm dedupe` now prune the `minimumReleaseAgeExclude` and `trustPolicyExclude` entries in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves, when `minimumReleaseAgeExcludePrune` or `trustPolicyExcludePrune` is enabled. Only `pnpm add`, `pnpm update`, and `pnpm remove` ran that cleanup before [pnpm/pnpm#14759](https://github.com/pnpm/pnpm/issues/14759).

- `ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` now names the file or directory in `node_modules` that pnpm could not clean up. It previously reported only the underlying OS error, such as "Access is denied (os error 5)".

- `pnpm sbom` now omits package author fields when the manifest author name is empty or contains only whitespace [pnpm/pnpm#14685](https://github.com/pnpm/pnpm/issues/14685). In a filtered or split workspace run, only a project with no `author` field inherits the workspace root's author.

- `pnpm sbom --sbom-format spdx` now writes `creationInfo.created` with whole seconds, such as `2026-09-08T10:38:21Z`. The timestamp carried fractional seconds, which strict SPDX consumers rejected [#14684](https://github.com/pnpm/pnpm/issues/14684).

- pnpm does less filesystem work when an install creates the command shims in `node_modules/.bin`. A warm install of a 76 project workspace makes about 1,500 fewer filesystem calls [#14540](https://github.com/pnpm/pnpm/issues/14540).

- Windows filesystem operations now retry permission errors for up to one second. Permanent permission errors previously delayed failure by a minute. Sharing and lock violations retain their one-minute retry budget [pnpm/pnpm#14682](https://github.com/pnpm/pnpm/issues/14682).

- `pnpm install` now runs a dependency's build scripts again when its side-effects cache entry has no files to restore. Such builds were skipped and nothing was put in their place, so a script whose whole effect lands outside its own package directory, such as a git-hook installer, never took effect. The same applies to an artifact from the shared side-effects cache, and pnpm no longer publishes such empty artifacts [#14717](https://github.com/pnpm/pnpm/issues/14717).

- pnpm now writes `node_modules/.package-map.json` only when `nodeExperimentalPackageMap` is enabled. Nothing reads the file without that setting. An install that stops writing the map removes the one a previous install left.

- The `updateConfig` pnpmfile hook now receives the resolved configuration, including settings that came from `.npmrc`, the command line, or a default. Scoped registries are reported under `registriesByScope`, and a hook may rewrite that map to change where packages are fetched from. Registry credentials are reported under `configByUri`, as pnpm 11 reports them. A setting nothing set is omitted rather than reported as `null` [#14676](https://github.com/pnpm/pnpm/issues/14676).

- `pnpm update <name>@<version>` now keeps the range operator the manifest already declares. Running `pnpm update react@19.3.0` on `"react": "^19.2.8"` writes `"react": "^19.3.0"` [#14745](https://github.com/pnpm/pnpm/issues/14745). A `jsr:` entry keeps its `jsr:` prefix as well, and a plain `pnpm update` now moves a `jsr:` range the way it moves an npm range.

- pnpm now passes Ctrl+C on to the script or command it started and waits for it to shut down. pnpm used to exit first, so a script that was still writing landed on the shell prompt [#14723](https://github.com/pnpm/pnpm/issues/14723).

- Sped up installs that use the global virtual store. The map of slot paths is cached in the cache directory instead of being derived on every install.

- pnpm now warns when the root `package.json` declares a non-empty `workspaces` array and the project has no `pnpm-workspace.yaml`. Such an install linked no project and said nothing about why [#2255](https://github.com/pnpm/pnpm/issues/2255).
