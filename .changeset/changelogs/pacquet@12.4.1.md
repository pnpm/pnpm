## 12.4.1

pnpm 12.4.1 fixes installs that failed on filesystems refusing hard links or clones, on Android, and under `nodeLinker: hoisted`. Repeat installs are faster.

### Patch Changes

#### Installing packages

- `pnpm install` no longer fails with `Operation not permitted` when the filesystem refuses a hard link or a copy-on-write clone [#14722](https://github.com/pnpm/pnpm/issues/14722). Under `packageImportMethod: auto` and `clone-or-copy`, pnpm copies the file instead. EdenFS checkouts, which have no hard links, and rootless containers, which refuse the clone syscall, both hit this. An explicit `packageImportMethod: hardlink` or `clone` still reports the error.

  pnpm also copies a package file whose store entry has reached the filesystem's limit on names for one file, 1024 on NTFS and 65000 on ext4. Such a file failed the install under `packageImportMethod: hardlink`, and under `auto` it stopped pnpm hard linking for the rest of the install.

- `pnpm install` no longer writes a package file through a symlink left at the path it is importing to. Copying such a file overwrote whatever the link pointed at, and created that file when the link pointed nowhere. An executable package file also made the link's target executable.

- Fixed `pnpm install` and `pnpm dlx` on Android. Registry requests crashed because pnpm found no system CA certificates, so pnpm uses bundled ones there [#14777](https://github.com/pnpm/pnpm/issues/14777). Imports also failed with "Permission denied" on filesystems that deny hard links and reflinks, and now fall back to copying [#14780](https://github.com/pnpm/pnpm/issues/14780).

- `pnpm install` no longer fails with "Invalid cross-device link" while preserving a package's nested `node_modules` directory during a Docker build [#14758](https://github.com/pnpm/pnpm/issues/14758).

- `pnpm install` no longer fails on a package tarball that carries a file at the archive root, such as the `._*` entries macOS `tar` adds [#14701](https://github.com/pnpm/pnpm/issues/14701). The file is installed at the root of the package.

  A `file:` tarball packed without the usual `package/` directory is now recorded under the name and version from its own `package.json`. It was recorded under the alias the dependency was given, at version 0.0.0.

- Under `nodeLinker: hoisted`, `pnpm install` no longer re-imports packages that are already in place. A repeat install replaced the whole `node_modules` tree and reported `Packages: +N`. A package is still imported when its directory is missing, when its `package.json` no longer carries the installed version, when it is a `file:` dependency, and when it is patched. Lifecycle scripts no longer run again for a package left in place, and `pnpm rebuild` and a change to `allowBuilds` still reach it.

- `pnpm install` now runs a dependency's build scripts again when its side-effects cache entry has no files to restore [#14717](https://github.com/pnpm/pnpm/issues/14717). Such builds were skipped and nothing was put in their place, so a script whose whole effect lands outside its own package directory, such as a git hook installer, never took effect. pnpm no longer publishes empty artifacts to the shared side-effects cache either.

#### Resolving and linking dependencies

- `pnpm install`, `pnpm add`, and `pnpm dedupe` now apply `ignoredOptionalDependencies` [#14729](https://github.com/pnpm/pnpm/issues/14729). Matching optional dependencies are left out of the lockfile and are not installed. pnpm 12 installed them whenever it resolved dependencies from scratch.

- `pnpm install` no longer links a transitive dependency to a workspace package when `linkWorkspacePackages` is `true` and the dependency is declared with a plain version range [#14781](https://github.com/pnpm/pnpm/issues/14781). Enabling `preferWorkspacePackages` does not change this. Set `linkWorkspacePackages: deep` to link them.

- `pnpm install` no longer leaves dangling dependency links in workspace packages located above the workspace root [#14726](https://github.com/pnpm/pnpm/issues/14726).

- `pnpm install` and `pnpm add` no longer leave a dangling symlink in `node_modules` when a project starts depending directly on a package that the lockfile holds only as a transitive dependency with resolved peer dependencies [#14714](https://github.com/pnpm/pnpm/issues/14714).

- `pnpm dedupe` now keeps a compatible auto-installed peer when another workspace project depends on a newer major [#14697](https://github.com/pnpm/pnpm/issues/14697). Repeated runs alternated between compatible and incompatible peer versions.

- `pnpm peers check` no longer reports a peer dependency declared as `workspace:^`, `workspace:~`, or a bare `workspace:` as unmet [#14770](https://github.com/pnpm/pnpm/issues/14770). pnpm reported these as unmet whatever version the linked workspace project supplied.

#### Performance

- Sped up repeat installs [#14540](https://github.com/pnpm/pnpm/issues/14540). pnpm checks the store's files only for the packages it links into `node_modules`, instead of every package in the lockfile. Creating the command shims in `node_modules/.bin` makes about 1,500 fewer filesystem calls in a 76 project workspace. Installs that use the global virtual store read their slot paths from the cache directory instead of deriving them every time. Verifying a large lockfile also allocates less memory.

- Sped up `pnpm install` in Cargo workspaces with many member crates. Repeated installs reuse verified Cargo checksum metadata.

- Installing several packages from the same Git repository and commit now downloads the source once per install [#14725](https://github.com/pnpm/pnpm/issues/14725). Each package still runs its prepare scripts in its own copy of the checkout.

#### Running scripts and tasks

- pnpm now passes Ctrl+C on to the script or command it started and waits for it to shut down [#14723](https://github.com/pnpm/pnpm/issues/14723). pnpm exited first, so a script that was still writing landed on the shell prompt.

- `pnpm run "/pattern/" --no-bail` now lets every matched script finish after one of them fails [#14718](https://github.com/pnpm/pnpm/issues/14718). The command exits with `ERR_PNPM_RUN_FAILED`, and its message lists the scripts that failed in the order they were selected.

- `pnpm pipeline` no longer fails on a project that tracks a symlink, such as a `CLAUDE.md` pointing at `AGENTS.md` [#14692](https://github.com/pnpm/pnpm/issues/14692). Changing a symlinked input's target invalidates that task's cache, and `pnpm pipeline --no-cache` no longer hashes task inputs.

#### Commands

- `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` no longer change global bins or install directories after reading only part of an installed package group [#13796](https://github.com/pnpm/pnpm/issues/13796). If any declared package manifest is missing, malformed, or unreadable, pnpm now fails before it activates or removes anything and leaves the existing global installation intact.

- `pnpm dedupe` now processes every workspace project by default, including workspaces that keep a separate lockfile per project [#14732](https://github.com/pnpm/pnpm/issues/14732). Workspace filters select which projects it processes, and `--fail-if-no-match` exits with an error when no project matches.

- `pnpm update <name>@<version>` now keeps the range operator the manifest declares [#14745](https://github.com/pnpm/pnpm/issues/14745). Running `pnpm update react@19.3.0` on `"react": "^19.2.8"` writes `"react": "^19.3.0"`. A `jsr:` entry keeps its `jsr:` prefix, and a plain `pnpm update` now moves a `jsr:` range the way it moves an npm range.

- `pnpm --filter` directory selectors now support `?` wildcards and character classes such as `[ab]`. A `*` or `?` wildcard no longer selects a directory whose name starts with a dot, as on pnpm 11.

- `pnpm deploy --legacy` now prefers the dependency versions pinned in the source workspace lockfile when they still satisfy the deployed project's range [#13857](https://github.com/pnpm/pnpm/issues/13857).

- `pnpm sbom` now leaves out a package's author field when the manifest author name is empty or contains only whitespace [#14685](https://github.com/pnpm/pnpm/issues/14685). In a filtered or split workspace run, only a project with no `author` field inherits the workspace root's author.

  `pnpm sbom --sbom-format spdx` now writes `creationInfo.created` with whole seconds, such as `2026-09-08T10:38:21Z` [#14684](https://github.com/pnpm/pnpm/issues/14684). The fractional seconds it carried were rejected by strict SPDX consumers.

#### Configuration

- The `updateConfig` pnpmfile hook now receives the resolved configuration, including settings that came from `.npmrc`, the command line, or a default [#14676](https://github.com/pnpm/pnpm/issues/14676). Scoped registries are reported under `registriesByScope`, and a hook may rewrite that map to change where packages are fetched from. Registry credentials are reported under `configByUri`, as pnpm 11 reports them. An unset setting is left out rather than reported as `null`.

- `pnpm audit --fix` and the `minimumReleaseAgeStrict` approval prompt now keep the comments in `minimumReleaseAgeExclude` when they append an entry to it in `pnpm-workspace.yaml`. The rest of the list is left as written, and the `trustPolicyExcludePrune` and `minimumReleaseAgeExcludePrune` cleanups keep the comments of the entries they retain.

  `pnpm install` and `pnpm dedupe` now run those cleanups too [#14759](https://github.com/pnpm/pnpm/issues/14759). Only `pnpm add`, `pnpm update`, and `pnpm remove` pruned the entries that the freshly written lockfile no longer resolves.

- `pnpm config set --global node-download-mirrors` no longer rejects the key [#13611](https://github.com/pnpm/pnpm/issues/13611). The global config file already accepted `nodeDownloadMirrors`, but the command refused to write it.

- `NO_PROXY` entries that start with a dot, such as `.npmjs.org`, now bypass the proxy for the domain and its subdomains [#14686](https://github.com/pnpm/pnpm/issues/14686).

- pnpm no longer creates a project `pnpm-lock.yaml` when `devEngines.packageManager.onFail` is `download` and lockfile writing is off through `lockfile: false` or `--no-lockfile` [#14728](https://github.com/pnpm/pnpm/issues/14728). pnpm still switches to the pinned version.

- pnpm now writes `node_modules/.package-map.json` only when `nodeExperimentalPackageMap` is enabled. Nothing reads the file without that setting, and an install that stops writing the map removes the one a previous install left.

#### Windows

- `pnpm pipeline` no longer fails with intermittent access denied errors when concurrent tasks save their cache entries on Windows.

- Windows filesystem operations now retry permission errors for up to one second [#14682](https://github.com/pnpm/pnpm/issues/14682). A permanent permission error delayed the failure by a minute. Sharing and lock violations keep their one minute retry budget.

#### Messages and output

- pnpm now warns when the root `package.json` declares a non-empty `workspaces` array and the project has no `pnpm-workspace.yaml` [#2255](https://github.com/pnpm/pnpm/issues/2255). Such an install linked no project and said nothing about why.

- `ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` now names the file or directory in `node_modules` that pnpm could not clean up. It reported only the underlying OS error, such as "Access is denied (os error 5)".

- `pnpm --help` no longer describes pnpm as experimental.
