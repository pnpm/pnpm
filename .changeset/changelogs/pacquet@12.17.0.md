## 12.17.0

pnpm 12.17.0 adds `pnpm peers check --global`, dependency-group selectors and wildcards in `overrides`, and runtime, package manager, Cargo, and Python updates in `pnpm outdated` and `pnpm update`. It also fixes files that other users could not read in Docker images, and repeat installs that missed changes to local dependencies.

### Minor Changes

- `pnpm peers check` now accepts `--global` (`-g`) to check globally installed packages. After a global install or update, the peer dependency warning now suggests `pnpm peers check -g` [#17504](https://github.com/pnpm/pnpm/issues/17504).

- `pnpm outdated` and `pnpm update` now inspect and update package manager and runtime versions in `devEngines.packageManager`, `packageManager`, `devEngines.runtime`, `engines.runtime`, and `runtime:` dependencies. With `cargo.enabled` or `python.enabled`, `pnpm outdated` and `pnpm update` also inspect and update Cargo dependencies in `Cargo.toml` and `Cargo.lock` and Python dependencies in `pyproject.toml` and `pylock.toml`.

- `overrides` selectors now support `:prod`, `:optional`, and `:peer` pseudo-selectors that target a single dependency group. They also support `*` wildcard patterns on parent and target package names. An override with a non-range value, such as `npm:` or `file:`, no longer adds a peer dependency to `dependencies`. Removing an optional dependency with `"-"` now also removes the duplicate entry from `dependencies` [#5391](https://github.com/pnpm/pnpm/issues/5391).

### Patch Changes

#### Installing packages

- Files that pnpm creates in a project now get the permissions the umask gives any new file, such as `0644`. Before, several of them were readable only by the user who ran pnpm, which broke Docker images that run as a different user [#17435](https://github.com/orgs/pnpm/discussions/17435).

  The fix applies to:

  - `node_modules/.pnpm-workspace-state-v1.json`. A different user running `pnpm run` reinstalled the dependencies because it could not read this file.
  - `pnpm-workspace.yaml`
  - the files written by `pnpm deploy`
  - patch files from `pnpm patch-commit`
  - the lockfile restored by `pnpm dedupe --check`
  - the Cargo and Python files that pnpm writes

  When pnpm edits an existing file, the file keeps its permissions.

- `pnpm install` no longer writes broken dependency links when the project's `node_modules` is a symlink to another directory, such as another project's `node_modules`. The install reported success, but the links it wrote did not resolve [#17448](https://github.com/pnpm/pnpm/issues/17448).

- Commands installed with `pnpm add --global` now find the dependencies that pnpm hoists for their package. A package that loads a dependency it does not declare, such as a native addon's platform-specific package, failed with "Cannot find module". Reinstall such a package with `pnpm add --global` to update its existing command [#17503](https://github.com/pnpm/pnpm/issues/17503).

- When pnpm switches to the version a project pins, it now installs that version's native binary for the current host, even if the project's `supportedArchitectures` excludes the host. Before, pnpm could not use the pinned version and kept running the installed one. pnpm also repairs a pinned version that an earlier install left without the host's binary.

- With the global virtual store, a change to a dependency of one package in a dependency cycle now gives every package in that cycle a new directory. Before, the other packages kept their directories and still linked the old copy of the changed package, so the project could load two copies of it [#17466](https://github.com/pnpm/pnpm/issues/17466).

  Packages in a dependency cycle, and packages that depend on one, move to new global virtual store directories once after this update. Those that run a build miss the side-effects cache once.

- With `nodeLinker: hoisted`, when two workspace projects need the same package version and it cannot be hoisted to the root, each project now keeps the copy installed in its own `node_modules`. Build scripts run in each of these copies. Before, the second project's copy was replaced with a symlink to the first project's, so every repeat install imported it again [#17498](https://github.com/pnpm/pnpm/issues/17498).

- The Node.js `latest` selector now chooses the newest stable release [#17490](https://github.com/pnpm/pnpm/issues/17490).

- Hardlinking from the store now stats each store file once per install. Since pnpm 12.8.0, a store file was stat'ed again for every package that links it. On an offline install of a large workspace, that added up to 15% system CPU [#17500](https://github.com/pnpm/pnpm/issues/17500).

#### Repeat installs and local dependencies

- `pnpm install --frozen-lockfile` no longer reports an outdated lockfile right after the install that wrote it. This happened when a pnpmfile `readPackage` hook or `packageExtensions` changed the dependencies or peer dependencies of a local directory dependency outside the workspace projects. It affected a `file:` dependency, an injected `workspace:../dir` dependency outside the workspace patterns, and an injected workspace package left out by `--filter` [#17447](https://github.com/pnpm/pnpm/issues/17447) [#17473](https://github.com/pnpm/pnpm/issues/17473).

  A repeat `pnpm install` now takes the fast path when `ignoredOptionalDependencies` removes an optional dependency of a `file:` directory dependency. It used to resolve again on every run.

- `pnpm install` now resolves again after an edit to an injected `workspace:../dir` dependency outside the workspace patterns under a shared lockfile. It used to keep the lockfile unchanged. The next `pnpm install --frozen-lockfile` then failed [#17476](https://github.com/pnpm/pnpm/issues/17476).

- `pnpm install` now resolves a local directory again after its `package.json` changes when an override or a package extension adds it to the dependencies of a registry package. `pnpm install --frozen-lockfile` fails on a lockfile that still records the old dependencies of such a directory [#17486](https://github.com/pnpm/pnpm/issues/17486).

- With `injectWorkspacePackages` and per-project lockfiles, `pnpm install` now updates a project's lockfile when a local tarball that an injected workspace project depends on is replaced. The project's lockfile kept the old version and integrity before.

- `pnpm install` now detects a change to a local tarball or directory that only a pnpmfile `readPackage` hook depends on. A repeat install reported "Already up to date" and kept the old contents before [pnpm/tasks#227](https://github.com/pnpm/tasks/issues/227).

- `pnpm install` now relinks a `workspace:` or `link:` dependency whose link was deleted from `node_modules`. It used to report "Already up to date" and leave the link missing [#17485](https://github.com/pnpm/pnpm/issues/17485).

- A repeat `pnpm install` reports "Already up to date" again when a pnpmfile `readPackage` hook changes the dependencies of a workspace project. Since 12.15.0, every such install ran in full [#17468](https://github.com/pnpm/pnpm/issues/17468).

  After an edit to a project's `package.json`, `pnpm install` and `verifyDepsBeforeRun` now run the pnpmfile's `readPackage` hook on the edited manifest before comparing it with the lockfile. They could report the project up to date when the hook rewrote the edited dependency to a specifier the lockfile did not record.

#### Adding and updating dependencies

- `pnpm update --latest` with package selectors now updates the dependencies of the selected packages within their declared ranges. Excluded packages and their dependencies keep their locked versions [#17444](https://github.com/pnpm/pnpm/issues/17444).

- `pnpm add <pkg>@<tag>` now uses a catalog entry set to that same dist-tag. The dependency is saved as `catalog:` and the entry stays as written [pnpm/tasks#225](https://github.com/pnpm/tasks/issues/225).

  When `pnpm add` adds a new dist-tag such as `vue@legacy` to a catalog, the catalog entry records the exact version the tag points to. The same applies to a dependency added without a version when the `tag` setting names a dist-tag other than `latest`.

- `pnpm add` now saves the resolved version range of an npm alias. For example, `pnpm add v2@npm:vue` and `pnpm add v2@npm:vue@^3.0.0` both save `npm:vue@^3.5.13`.

#### Running scripts and commands

- Programs that a script or `pnpm exec` command leaves running, such as a browser opened by the `open` package, now outlive pnpm exactly when they would outlive the command run without pnpm. Before, on Windows, a failed command, `Ctrl+C`, or a closed terminal ended all of them. pnpm still ends them when it is killed or crashes.

  When a recursive or parallel run bails, the scripts still running now receive `SIGTERM` on Unix and are killed only if they are still running two seconds later. On Windows, everything those scripts started is ended, including detached processes whose parent has already exited. Before, a bail killed them at once [#17465](https://github.com/pnpm/pnpm/issues/17465).

- On Windows, the shell shim of a `.cmd` or `.bat` bin now runs it under Git Bash when MSYS argument conversion is off, such as with `MSYS_NO_PATHCONV=1` or `MSYS2_ARG_CONV_EXCL='*'`. The shim used to start an interactive cmd.exe prompt [#17480](https://github.com/pnpm/pnpm/issues/17480).

- `pnx` and `pnpm create` now run the command in the directory selected by `--dir` or `--workspace-root`. Without either flag, the command still runs in the directory pnpm was started in.

- `pnpm --env-file` now reads files the way `node --env-file` does. Values are taken as written, so `$VAR` is no longer expanded and Windows paths such as `C:\Users\me` load unchanged. A file is no longer rejected for its contents.

#### Settings and messages

- `pnpm version` no longer creates a git commit and tag when `gitTagVersion` is set to `false` [#17460](https://github.com/pnpm/pnpm/issues/17460).

- `pnpm publish` now reads the `publishBranch` setting from `pnpm-workspace.yaml` and from `PNPM_CONFIG_PUBLISH_BRANCH`. The `--publish-branch` flag overrides it.

- `pnpm` now accepts `--strict-ssl` and `--local-address <ip>` on the command line.

- `pnpm doctor` uses a 15-second timeout when checking registry connectivity.

- `pnpm-workspace.yaml` parse errors now report `ERR_PNPM_INVALID_SETTING`.

- The error for a dependency with an invalid name in a project's manifest now names the project's directory. It used to say only "The current package", which did not tell which workspace project to fix [#17475](https://github.com/pnpm/pnpm/issues/17475).
