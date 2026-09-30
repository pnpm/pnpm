## 11.28.3

pnpm 11.28.3 updates `undici` to clear a security advisory, fixes "database disk image is malformed" errors when several pnpm processes share a store, and makes packages, catalogs, projects, and commands named like `constructor` work.

### Patch Changes

#### Installing packages

- pnpm now ships `undici` 7.29.1, so security scans of pnpm no longer report [GHSA-3wwx-pv8p-q78v](https://github.com/advisories/GHSA-3wwx-pv8p-q78v).

- pnpm no longer fails with "database disk image is malformed" or reads stale store entries while another pnpm process writes to the same store.

- Names that match built-in JavaScript object properties, such as `constructor`, `toString`, or `__proto__`, now work like any other name. pnpm crashed, wrote a wrong lockfile, or silently skipped such names in:

  - `pnpm add`, `pnpm install`, and `pnpm import`, for dependencies, peer dependencies, and `file:` dependencies that point to a directory named `constructor`.
  - Catalog entries and catalog names. Pruning unused entries crashed, and a new catalog named `toString` was not written.
  - Workspace projects, project directories, and files inside injected packages.
  - Registry prefixes and override version references such as `$toString`.
  - Hoisting, `pnpm list`, and `pnpm why`.
  - Command names. `pnpm constructor` runs the `constructor` script like any other unknown command, and `pnpm help constructor` no longer crashes.
  - Resolving through a pnpr server when a project lives in a directory named `constructor`.

- `pnpm install` no longer re-resolves an up-to-date lockfile on every run when a patched package is a peer in a peer cycle [#16418](https://github.com/pnpm/pnpm/issues/16418).

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build, where the system default path holds none of the utilities they call. Installing again replaces the shims already in `node_modules` [#16377](https://github.com/pnpm/pnpm/issues/16377).

- In a project that pins another pnpm version, pnpm now passes a command with an option it does not know to the pinned version. Before, `pnpm install --auto-dedupe` failed with "Unknown option" even though the pinned pnpm supports it [#16353](https://github.com/pnpm/pnpm/issues/16353).

- pnpm now fails with `ERR_PNPM_INVALID_ALLOW_BUILDS` when `allowBuilds` is not an object or one of its values is not `true`, `false`, or a string. Such values used to be ignored silently.

- Removing a dependency whose bins are declared through `directories.bin` no longer leaves broken shims in `node_modules/.bin`.

- A custom resolver's `shouldRefreshResolution` hook that rejects no longer crashes pnpm with an unhandled rejection when another hook has already asked for a refresh.

#### Updating dependencies

- When `minimumReleaseAge` hides the version that `latest` points to, pnpm now falls back to a prerelease of the same major before a stable version of an older major. A stable version of the same major is still preferred. For example, while a new `1.0.0` is too new, pnpm picks `1.0.0-beta.4` rather than an old `0.0.1` [#16388](https://github.com/pnpm/pnpm/issues/16388).

- `pnpm --filter <project> update <pkg>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when the selected projects do not depend on `<pkg>`, also in a workspace with a shared lockfile and a root project. It used to exit successfully.

- `pnpm audit --fix` now updates vulnerable packages in a single project that sets `updateConfig.ignoreDependencies`. It used to leave them on the vulnerable version.

- `pnpm update --global` now removes hard-linked executables from `PNPM_HOME` when migrating packages from the old global layout [#16420](https://github.com/pnpm/pnpm/issues/16420).

- Updating a pinned GitHub Action now rewrites the version in its `# vX.Y.Z` comment even when the action name contains the same version text. The action name used to change while the comment kept the old version.

#### Workspaces and deploy

- `pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` in a workspace with `injectWorkspacePackages: true` when a workspace package also lists its peer dependency as a dev dependency [#16375](https://github.com/pnpm/pnpm/issues/16375).

- `pnpm deploy` no longer copies the workspace root's `packageManager` and `devEngines.packageManager` fields into the deployed `package.json` [#16403](https://github.com/pnpm/pnpm/issues/16403).

- `--filter` fixes:

  - A `...pkg...` selector combined with another dependents selector, such as `--filter ...a --filter ...b...`, no longer adds the dependencies of the other selector's dependents.
  - `--filter "[<since>]"` now detects changes in projects whose directory names contain non-ASCII characters. The change used to be credited to the parent project.

#### Running scripts

- After relaying a signal to a script, pnpm keeps waiting for a process in the script's process group whose main thread has exited while its other threads still run. Linux reports such a process as a zombie, so the wait used to end before those threads finished [pnpm/tasks#56](https://github.com/pnpm/tasks/issues/56).

- A lifecycle script run with `unsafePerm: false` now fails with an error when pnpm cannot create `node_modules/.tmp`. It used to hang.

- `pnpm run` with `verifyDepsBeforeRun` no longer crashes with an unhandled rejection when a lockfile it did not need to compare fails to load.

- `pnpm run -r` now closes the collapsible CI log section of a project whose script fails, so the output of later projects is no longer nested inside it.

- `pnpm run --resume-from` no longer crashes when a saved run state file contains `null`.

#### Store

- `pnpm store prune` now removes the packages that only expired `pnpm dlx` cache entries used, as long as the store still has another registered project. They used to stay until the next `pnpm store prune` [#16383](https://github.com/pnpm/pnpm/discussions/16383).

- `pnpm store prune` now stops with an error when it cannot read a project directory for a reason other than the directory missing, such as a permission error. It used to skip the directory.

#### Publishing and registry output

- `pnpm publish` now includes bare `README` files and README files with Markdown extensions such as `readme.markdown` in registry metadata [#12704](https://github.com/pnpm/pnpm/issues/12704).

- `pnpm pack-app` now accepts an entry file or output directory inside the project whose name starts with two dots, such as `..build/entry.cjs`. It used to fail with `ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT`.

- Registry error messages now always say "(response body truncated)" when pnpm cut the response body short. The marker was missing when the body was cut at exactly 64 KiB.
