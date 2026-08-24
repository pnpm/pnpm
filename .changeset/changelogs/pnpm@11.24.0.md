## 11.24.0

### Minor Changes

- Added global build approvals [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

### Patch Changes

- Fixed pnpm v11 incorrectly reporting `confirmModulesPurge` as unrecognized when set in `pnpm-workspace.yaml`. The Rust CLI now identifies the unsupported option as a pnpm v11 setting instead of suggesting an unrelated setting.

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when the pinned pnpm version recorded in `pnpm-lock.yaml` has to be re-resolved before it can be installed. It runs the pnpm version the lockfile pins and leaves the lockfile unchanged [#14124](https://github.com/pnpm/pnpm/issues/14124).

- Under `nodeLinker: hoisted`, peer-resolution variants of an injected directory dependency (a `file:` snapshot) are materialized as separate copies again instead of collapsing onto the first-seen variant. Each copy keeps its own peer-resolved dependency set, so a project pinning one peer version no longer resolves another project's variant — Bit root components with conflicting peers across injected copies rely on this.

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a branch lockfile predates the removal of a dependency, or its move to another dependency group [#13966](https://github.com/pnpm/pnpm/issues/13966). A dependency that no project declares anymore is no longer reinstated by the merge, and the packages it was the only path to are dropped with it.

- Batch workspace publishing accepts a shared scope-specific credential, rejects mismatched credentials for a registry before publishing, and runs the `publish` and `postpublish` scripts after each completed registry group [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- The Rust CLI now honors five settings it recognized but ignored: `updateNotifier`, `legacyDirFiltering`, `initAuthorName` / `initAuthorEmail` / `initAuthorUrl`, `initLicense`, and `initVersion`. `pnpm install` and `pnpm add` check once a day for a newer pnpm and print how to get it (turn it off with `updateNotifier: false`); a `{<dir>}` filter selector can go back to matching the subtree below the directory with `legacyDirFiltering: true`; and `pnpm init` writes the configured author, license, and version into the `package.json` it scaffolds. `PNPM_CONFIG_INIT_VERSION` is now read as well.

  `maxsockets`, npm's spelling of `maxSockets`, is no longer ignored: both spellings are read from `pnpm-workspace.yaml`, the global config file, the environment, and the command line, in that increasing order of precedence — a value passed on the command line now wins even when the two sides spelled the setting differently.

  A `lastUpdateCheck` timestamp dated in the future — after a clock change, a restored snapshot, or a hand-edited state file — no longer silences the update check until that time comes around.

  `legacyDirFiltering` no longer reaches the workspace-root selectors pnpm generates for itself: the `!{<workspace-root>}` exclusion a recursive `run` / `exec` / `add` / `test` appends, and the `{<workspace-root>}` inclusion `--workspace-root` appends. Read as subtree matches they named every project below the root, so a recursive command under the setting selected nothing at all, and `--workspace-root` pulled in every project below the root instead of the root alone [#14101](https://github.com/pnpm/pnpm/issues/14101).

- `pnpm install --frozen-lockfile` no longer fails when `pnpm-lock.yaml` records the pinned pnpm version alongside an engine package the running pnpm does not install it from. An entry pinning another version is still refused, and a plain install rewrites the block [#14124](https://github.com/pnpm/pnpm/issues/14124).
