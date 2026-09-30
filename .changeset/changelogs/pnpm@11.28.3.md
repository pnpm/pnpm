## 11.28.3

### Patch Changes

- `pnpm add constructor` now adds the `constructor` package like any other dependency. pnpm used to read built-in object properties as its previous specifier and dependency type.

- `pnpm audit --fix` now updates vulnerable packages in a single project that sets `updateConfig.ignoreDependencies`. It used to leave them on the vulnerable version.

- Catalogs now work with entries and catalog names such as `constructor` or `toString`. Pruning unused catalog entries crashed on such names, and a new named catalog called `toString` was silently not written.

- A registry prefix or an override version reference named like a built-in object property, such as `constructor` or `$toString`, is now handled correctly. The prefix was rejected as declared by two registries, and the override resolved to a function.

- A custom resolver's `shouldRefreshResolution` hook that rejects no longer crashes pnpm with an unhandled rejection when another hook has already asked for a refresh.

- `pnpm deploy` no longer copies the workspace root's `packageManager` and `devEngines.packageManager` fields into the deployed `package.json` [#16403](https://github.com/pnpm/pnpm/issues/16403).

- `pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` in a workspace with `injectWorkspacePackages: true` when a workspace package lists its peer dependency as a dev dependency too [#16375](https://github.com/pnpm/pnpm/issues/16375).

- `pnpm install` no longer crashes or writes a wrong lockfile when a dependency, peer dependency, or catalog entry is named `constructor`.

- `pnpm install` no longer crashes when a `file:` dependency points to a directory named `constructor`.

- `pnpm run` with `verifyDepsBeforeRun` no longer crashes with an unhandled rejection when a lockfile it did not need to compare fails to load.

- `pnpm store prune` now removes the packages that only expired `pnpm dlx` cache entries used, as long as the store still has another registered project. They were left in the store until the next `pnpm store prune` [#16383](https://github.com/pnpm/pnpm/discussions/16383).

- `--filter` fixes:

  - A `...pkg...` selector combined with another dependents selector, such as `--filter ...a --filter ...b...`, no longer adds the dependencies of the other selector's dependents.
  - `--filter "[<since>]"` now detects changes in projects whose directory names contain non-ASCII characters. The change used to be credited to the parent project.

- `pnpm --filter <project> update <pkg>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when the selected projects do not depend on `<pkg>`, also in a workspace with a shared lockfile and a root project. It used to install and exit successfully.

- `ReadOnlyStoreIndex` now observes concurrent writes to a shared store safely. Concurrent writes could previously cause "database disk image is malformed" errors or stale reads. Callers reading a finalized store on a read-only filesystem must use `ImmutableStoreIndex`. The `frozenStore` setting continues to use immutable access.

- `pnpm update --global` now removes hard-linked executables from `PNPM_HOME` when migrating packages from the old global layout [pnpm/pnpm#16420](https://github.com/pnpm/pnpm/issues/16420).

- Enable `zlib-rs` feature for decompression backend to improve package extraction performance.

- Updating a pinned GitHub Action now rewrites the version in its `# vX.Y.Z` comment even when the action name contains the same version text. The action name was changed and the comment kept the old version.

- A direct dependency named `constructor` of a workspace project is now hoisted like any other dependency.

- `pnpm import` no longer crashes when the imported lockfile or a Yarn patch refers to a package named `constructor`.

- When `minimumReleaseAge` hides the version that `latest` points to, pnpm now falls back to a prerelease of the same major before a stable version of an older major. A stable version of the same major is still preferred. Previously, while a new `1.0.0` was too new, `latest` fell back to an old `0.0.1` even though `1.0.0-beta.4` had been `latest` until then [#16388](https://github.com/pnpm/pnpm/issues/16388).

- A lifecycle script run with `unsafePerm: false` now fails with an error when pnpm cannot create `node_modules/.tmp`. It used to hang.

- `pnpm list` now shows an unsaved dependency named `constructor`, and `pnpm why` no longer labels a dependency aliased `constructor` as a dev dependency.

- `pnpm pack-app` now accepts an entry file or output directory inside the project whose name starts with two dots, such as `..build/entry.cjs`. It used to fail with `ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT`.

- `pnpm install` no longer re-resolves an up-to-date lockfile on every run when a patched package is a peer in a peer cycle [#16418](https://github.com/pnpm/pnpm/issues/16418).

- Resolving through a pnpr server no longer fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH` when a workspace project lives in a directory named `constructor`.

- `pnpm constructor`, `pnpm help constructor`, and similar commands named like built-in object properties no longer crash. `pnpm constructor` runs the `constructor` script like any other unknown command.

- Workspaces now work with a dependency or project named `constructor`, a project directory named `__proto__`, and injected packages that contain files named like `constructor` or `valueOf`. pnpm crashed on some of these names and silently skipped others.

- `pnpm store prune` now aborts when an error other than a missing directory occurs while scanning project directories in the mark phase.

- `pnpm publish` now includes bare `README` files and README files with Markdown extensions such as `readme.markdown` in registry metadata [#12704](https://github.com/pnpm/pnpm/issues/12704).

- Registry error messages now always say "(response body truncated)" when pnpm cut the response body short. The marker was missing when the body was cut at exactly 64 KiB.

- Removing a dependency whose bins are declared through `directories.bin` no longer leaves broken shims in `node_modules/.bin`. pnpm now removes the bins before it deletes the package directory.

- `pnpm run -r` now closes the collapsible CI log section of a project whose script fails, so the output of later projects is no longer nested inside it. `--resume-from` no longer crashes when a saved run state file contains `null`.

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build, where the system default path holds none of the utilities they call. Installing again replaces the shims already in `node_modules` [#16377](https://github.com/pnpm/pnpm/issues/16377).

- pnpm now ships `undici` 7.29.1, so security scans of pnpm no longer report [GHSA-3wwx-pv8p-q78v](https://github.com/advisories/GHSA-3wwx-pv8p-q78v).

- In a project that pins another pnpm version, pnpm now passes a command with an option it does not know to the pinned version. Before, pnpm rejected the option before switching, so `pnpm install --auto-dedupe` failed with "Unknown option" even though the pinned pnpm supports it [#16353](https://github.com/pnpm/pnpm/issues/16353).

- pnpm now fails with `ERR_PNPM_INVALID_ALLOW_BUILDS` when `allowBuilds` is not an object or one of its values is not `true`, `false`, or a string. Such values used to be ignored silently.

- After relaying a signal to a script, pnpm keeps waiting for a process in the script's process group whose main thread has exited while its other threads still run. Linux reports such a process as a zombie, so the wait used to end before those threads finished [pnpm/tasks#56](https://github.com/pnpm/tasks/issues/56).
