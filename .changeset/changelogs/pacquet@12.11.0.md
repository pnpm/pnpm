## 12.11.0

### Minor Changes

- `pnpm install` now links the agent skills that direct dependencies ship under `skills/<name>/SKILL.md` into the project's agent skill directories, such as `.claude/skills`. A package's skills are linked only after you approve them with `pnpm approve`. The `skills.dirs` setting chooses the directories [pnpm/rfcs#35](https://github.com/pnpm/rfcs/pull/35).

- With `cargo.enabled`, `pnpm install` now installs the Rust toolchain named in `rust-toolchain.toml`. pnpm verifies the release signature, stores the toolchain once per machine, and links it into `.pnpm/rust`. `pnpm run` and `pnpm exec` put its `cargo` and `rustc` on the `PATH`.

- `pnpm run` and `pnpm exec` now keep the colors of script output that they print under the project's name, such as with `--stream`. pnpm sets `FORCE_COLOR=1` for these scripts when its own output is in color, unless `FORCE_COLOR` is already set.

  Script output is also rendered more cleanly:

  - A line that a progress bar redraws with `\r` shows only its last state.
  - Escape codes that move the cursor or clear the screen are dropped.
  - Long colored lines are cut at the terminal width.

- Added the `permissions` setting, which records what each dependency may do. Its `build` capability works like `allowBuilds` and takes precedence over it. `pnpm approve-builds` writes to `permissions` when `pnpm-workspace.yaml` already has it, and to `allowBuilds` otherwise.

  Added `pnpm permissions`, which lists the granted and denied permissions and the packages awaiting approval. `pnpm approve` reviews build scripts and agent skills in one prompt [pnpm/rfcs#36](https://github.com/pnpm/rfcs/pull/36).

- `pnpm add -g rust@<channel>` installs a Rust toolchain globally. The `cargo` and `rustc` commands run it outside projects that pin their own. `pnpm update -g`, `pnpm ls -g`, and `pnpm remove -g` manage it like any global package. In a project, `pnpm add rust@<channel>` pins the toolchain in `rust-toolchain.toml`.

- `pnpm shim add rust` adds project-aware shims for `cargo`, `rustc`, and the other Rust tools. In a project with a `rust-toolchain.toml`, they run the toolchain the file names and install it on first use. Elsewhere, the next command of the same name on `PATH` runs, such as rustup's.

### Patch Changes

- `pnpm access` now resolves target registries for scoped packages and scopes from configuration, and honors the `--registry` option across all subcommands.

- `pnpm audit --fix update` now updates only the dependencies whose locked version is vulnerable [#14928](https://github.com/pnpm/pnpm/issues/14928).

- `pnpm install --no-optional` now installs the peer dependencies a project declares when `autoInstallPeers` is on. The lockfile marked such a peer `optional: true` when another dependency had it as an optional peer.

- When the `pnpm` package has to download its native binary on first run, it now uses the registry and credentials from `.npmrc` and from the `npm_config_registry` and `pnpm_config_registry` environment variables. `COREPACK_NPM_REGISTRY` still takes precedence. A project `.npmrc` is not read when `COREPACK_INTEGRITY_KEYS` turns off the signature check [#16655](https://github.com/pnpm/pnpm/issues/16655).

- `pnpm install` now writes the source replacement for vendored crates into `.pnpm/crates/config.toml` and includes it as optional from `.cargo/config.toml`. A checkout without `.pnpm` builds with plain Cargo [#16659](https://github.com/pnpm/pnpm/issues/16659).

- `pnpm remove` with `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).

- `pnpm star`, `pnpm unstar`, `pnpm stars`, `pnpm docs`, and `pnpm repo` now accept a `--registry` option. `pnpm star`, `pnpm unstar`, `pnpm stars`, `pnpm owner`, and `pnpm team` now preserve registry path prefixes when constructing request endpoints.

- `pnpm install` and `pnpm add --config` now apply `minimumReleaseAge` when they resolve a config dependency. A config dependency range resolves to the newest version that is old enough, so a later clean `pnpm install --frozen-lockfile` accepts the lockfile [#16660](https://github.com/pnpm/pnpm/issues/16660).

- Patches saved with CRLF line endings now apply, including a patch that creates or deletes a file. pnpm used to reject the git headers of such a patch with `ERR_PNPM_INVALID_PATCH` and the message `invalid file mode: 100644` [#16641](https://github.com/pnpm/pnpm/issues/16641).

- `pnpm deploy` with a shared lockfile no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` for `settings.dedupeInjectedDeps` or `settings.dedupePeerDependents` when `lockfile.includeResolutionSettings` is enabled. The deployed lockfile now records both settings as `false`, the values the deploy installs with.

- `pnpm deploy` now writes the dependencies in the deployed `package.json` sorted by name. It also sorts the `allowBuilds` entries in the deployed `pnpm-workspace.yaml`. Repeated deploys of the same lockfile now produce identical files [#16687](https://github.com/pnpm/pnpm/issues/16687).

- Fixed pnpm panicking with "unexpected error when polling the I/O driver" when it runs under QEMU user-mode emulation, such as a `linux/amd64` container on an Apple Silicon Mac [#16696](https://github.com/pnpm/pnpm/issues/16696).

- Fixed intermittent hoist link errors on Windows during concurrent junction creation or replacement [pnpm/tasks#53](https://github.com/pnpm/tasks/issues/53).

- `pnpm install` hardlinks files from a group-writable or world-writable store again. Before this fix, pnpm copied every file from such a store into `node_modules` [#16677](https://github.com/pnpm/pnpm/issues/16677).

- With `sharedWorkspaceLockfile: false`, an injected workspace package installed in the same run as its dependent now holds only the files its `files` field selects. The copy used to also hold other files of the project, such as `tsconfig.json` [#16683](https://github.com/pnpm/pnpm/issues/16683).

- A script listed in `syncInjectedDepsAfterScripts` no longer fails when it rewrites `package.json` while pnpm is copying its edits into the injected copies. The sync after the script now replaces a half-copied manifest.

- A repeat `pnpm install` no longer imports patched packages and packages with build scripts again when nothing changed. Their builds no longer run again either. This happened when `recursiveInstall` was `false` or the project had a `file:` dependency [#16705](https://github.com/pnpm/pnpm/issues/16705).

- `pnpm add` and other installs that re-resolve dependencies now keep the locked `devEngines.runtime` version while it still satisfies the declared range [#16764](https://github.com/pnpm/pnpm/issues/16764).

- `pnpm -r run` no longer garbles its live output when a script fails while other scripts are still running. The `[ELIFECYCLE]` lines that pnpm printed for the failed scripts shifted the lines it was redrawing.

- With `nodeLinker.type` set to `loaded`, Node.js now stops with `ERR_PNPM_LOADER_UNSUPPORTED_NODE` when it preloads the store loader on a version the loader cannot serve. The supported versions are `^24.18.0 || >=26.2.0`. On other versions, CommonJS packages imported from ESM failed with `Cannot find module` on their first relative `require()`.

- Verifying the lockfile against `trustPolicy` and `minimumReleaseAge` uses less memory. pnpm no longer keeps every published version's manifest of each checked package in memory until the install ends [#16656](https://github.com/pnpm/pnpm/issues/16656).

- `pnpm pack` and `pnpm publish` now match `.npmignore` and `.gitignore` rules the way npm does. A negation such as `!lib/**` or `!lib/**/!(*.map)` re-includes files under a directory that an earlier `*` rule excluded [#16743](https://github.com/pnpm/pnpm/issues/16743).

- `pnpm outdated` and `pnpm update --interactive` now apply `overrides` before they look up the latest version. Before, a dependency overridden to an npm alias was compared with the latest version of the package the override replaces [#16719](https://github.com/pnpm/pnpm/issues/16719).

- `pnpm pack` and `pnpm publish` no longer always include root files that merely start with `README`, `LICENSE`, or `LICENCE`, such as `README_INTERNAL.md`. Only `README`, `LICENSE`, `LICENCE`, and `COPYING`, with or without an extension, ship regardless of `files` and `.npmignore`, as in npm [#16753](https://github.com/pnpm/pnpm/issues/16753).

- A patch that changes a file's mode, such as adding or removing the executable bit, now applies the new mode on Unix [pnpm/tasks#110](https://github.com/pnpm/tasks/issues/110).

- `pnpm publish --provenance=false` and `--no-provenance` now turn off provenance under trusted publishing, so a package can be published from a self-hosted runner. Setting `provenance: false` in `pnpm-workspace.yaml` does the same [#16721](https://github.com/pnpm/pnpm/issues/16721).

- `pnpm rebuild <pkg>` and `pnpm rebuild --pending` no longer read the manifest of every installed package to find build scripts. Only the selected packages are inspected and built. On a large `node_modules` served lazily, such as over a network or FUSE mount, this turned a rebuild of a few packages into a fetch of every package.

- `pnpm rebuild` no longer removes and recreates `node_modules` when the settings recorded in `node_modules/.modules.yaml` differ from the current configuration. The rebuild runs the build scripts against the installed packages as they are.

- `pnpm ping`, `pnpm search`, and `pnpm access` now preserve registry path prefixes when constructing request endpoints.

- Registry commands such as `pnpm whoami`, `pnpm star`, and `pnpm team` now preserve registry path prefixes when constructing request endpoints. `pnpm view` now honors configured network retry settings.

- `pnpm bugs` now honors the `--registry` option when fetching package metadata from a registry.

- Removal overrides such as `"debug>supports-color": "-"` now also apply to an optional peer that a package declares only in `peerDependenciesMeta` [#16681](https://github.com/pnpm/pnpm/issues/16681).

- `pnpm repo` now fetches repository URLs for multiple packages in parallel. `pnpm repo` and `pnpm view` now honor the `--registry` option for scoped packages.

- `pnpm star`, `pnpm unstar`, `pnpm owner`, `pnpm deprecate`, `pnpm undeprecate`, `pnpm unpublish`, `pnpm dist-tag`, `pnpm team`, and `pnpm stage` now honor the `--registry` option for scoped packages.

- `pnpm store prune` now compacts the store's `index.db` after removing package entries, so the file shrinks again [#16717](https://github.com/pnpm/pnpm/issues/16717).

- pnpm now switches to the version a project pins in `packageManager` or `devEngines.packageManager` even when `pnpm-workspace.yaml` has a setting the running pnpm cannot read, such as a `lockfile.includeResolutionSettings` section. If pnpm does not switch, it still reports that setting [#16675](https://github.com/pnpm/pnpm/issues/16675).

- `pnpm view`, `pnpm update`, and other commands that read registry metadata now work behind proxies that end a response by closing the connection without a TLS `close_notify` alert [#16704](https://github.com/pnpm/pnpm/issues/16704).

- `pnpm whoami` now supports the `--registry` option to query credentials and identity against a specific registry.

- On Windows, rename invalid package filenames when an install encounters them.

- `pnpm install` and `pnpm dedupe` now link an optional peer to the workspace package that the workspace root depends on when the picked version matches it. Previously they installed the registry package with the same name and version [pnpm/pnpm#16706](https://github.com/pnpm/pnpm/issues/16706).
