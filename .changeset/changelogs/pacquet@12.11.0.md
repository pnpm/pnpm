## 12.11.0

This release adds Rust toolchain management, links the agent skills that dependencies ship, adds the `permissions` setting, and keeps the colors of streamed script output.

### Minor Changes

- `pnpm install` now links the agent skills that direct dependencies ship under `skills/<name>/SKILL.md` into the project's agent skill directories, such as `.claude/skills`. A package's skills are linked only after you approve them with `pnpm approve`. The `skills.dirs` setting chooses the directories [pnpm/rfcs#35](https://github.com/pnpm/rfcs/pull/35).

  Added the `permissions` setting, which records what each dependency may do. Its `build` capability works like `allowBuilds` and takes precedence over it. `pnpm approve-builds` writes to `permissions` when `pnpm-workspace.yaml` already has it, and to `allowBuilds` otherwise.

  Added `pnpm permissions`, which lists the granted and denied permissions and the packages awaiting approval. `pnpm approve` reviews build scripts and agent skills in one prompt [pnpm/rfcs#36](https://github.com/pnpm/rfcs/pull/36).

- pnpm now installs and runs Rust toolchains.

  - With `cargo.enabled`, `pnpm install` installs the toolchain named in `rust-toolchain.toml`. pnpm verifies the release signature, stores the toolchain once per machine, and links it into `.pnpm/rust`. `pnpm run` and `pnpm exec` put its `cargo` and `rustc` on the `PATH`.
  - `pnpm add -g rust@<channel>` installs a toolchain globally. The `cargo` and `rustc` commands run it outside projects that pin their own. `pnpm update -g`, `pnpm ls -g`, and `pnpm remove -g` manage it like any global package.
  - In a project, `pnpm add rust@<channel>` pins the toolchain in `rust-toolchain.toml`.
  - `pnpm shim add rust` adds project-aware shims for `cargo`, `rustc`, and the other Rust tools. In a project with a `rust-toolchain.toml`, they run the toolchain the file names and install it on first use. Elsewhere, the next command of the same name on `PATH` runs, such as rustup's.

- `pnpm run` and `pnpm exec` now keep the colors of script output that they print under the project's name, such as with `--stream`. pnpm sets `FORCE_COLOR=1` for these scripts when its own output is in color, unless `FORCE_COLOR` is already set.

  Script output is also rendered more cleanly:

  - A line that a progress bar redraws with `\r` shows only its last state.
  - Escape codes that move the cursor or clear the screen are dropped.
  - Long colored lines are cut at the terminal width.
  - `pnpm -r run` no longer garbles its live output when a script fails while other scripts are still running.

### Patch Changes

#### Installing packages

- pnpm no longer panics with "unexpected error when polling the I/O driver" when it runs under QEMU user-mode emulation, such as a `linux/amd64` container on an Apple Silicon Mac [#16696](https://github.com/pnpm/pnpm/issues/16696).

- `pnpm view`, `pnpm update`, and other commands that read registry metadata now work behind proxies that end a response by closing the connection without a TLS `close_notify` alert [#16704](https://github.com/pnpm/pnpm/issues/16704).

- pnpm now switches to the version a project pins in `packageManager` or `devEngines.packageManager` even when `pnpm-workspace.yaml` has a setting the running pnpm cannot read, such as a `lockfile.includeResolutionSettings` section. If pnpm does not switch, it still reports that setting [#16675](https://github.com/pnpm/pnpm/issues/16675).

- When the `pnpm` package has to download its native binary on first run, it now uses the registry and credentials from `.npmrc` and from the `npm_config_registry` and `pnpm_config_registry` environment variables. `COREPACK_NPM_REGISTRY` still takes precedence. A project `.npmrc` is not read when `COREPACK_INTEGRITY_KEYS` turns off the signature check [#16655](https://github.com/pnpm/pnpm/issues/16655).

- `pnpm install` and `pnpm add --config` now apply `minimumReleaseAge` when they resolve a config dependency. A config dependency range resolves to the newest version that is old enough, so a later clean `pnpm install --frozen-lockfile` accepts the lockfile [#16660](https://github.com/pnpm/pnpm/issues/16660).

- `pnpm remove` with `catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).

- With `cargo.enabled`, `pnpm install` now writes the source replacement for vendored crates into `.pnpm/crates/config.toml` and includes it as optional from `.cargo/config.toml`. A checkout without `.pnpm` builds with plain Cargo [#16659](https://github.com/pnpm/pnpm/issues/16659).

- With `nodeLinker.type` set to `loaded`, Node.js now stops with `ERR_PNPM_LOADER_UNSUPPORTED_NODE` when it preloads the store loader on a version the loader cannot serve. The supported versions are `^24.18.0 || >=26.2.0`. On other versions, CommonJS packages imported from ESM failed with `Cannot find module` on their first relative `require()`.

- On Windows, `pnpm install` no longer fails with "The filename, directory name, or volume label syntax is incorrect" when a package contains a file whose name is invalid on Windows, such as `icon.svg?as=metadata.d.ts`. pnpm removes the invalid characters from the name and prints a warning that lists the renamed files.

- On Windows, hoisting no longer fails intermittently with link errors when a junction is created or replaced concurrently [pnpm/tasks#53](https://github.com/pnpm/tasks/issues/53).

#### Resolving dependencies

- `pnpm install --no-optional` now installs the peer dependencies a project declares when `autoInstallPeers` is on. The lockfile marked such a peer `optional: true` when another dependency had it as an optional peer.

- `pnpm install` and `pnpm dedupe` now link an optional peer to the workspace package that the workspace root depends on when the picked version matches it. They installed the registry package with the same name and version [#16706](https://github.com/pnpm/pnpm/issues/16706).

- Removal overrides such as `"debug>supports-color": "-"` now also apply to an optional peer that a package declares only in `peerDependenciesMeta` [#16681](https://github.com/pnpm/pnpm/issues/16681).

- `pnpm add` and other installs that re-resolve dependencies now keep the locked `devEngines.runtime` version while it still satisfies the declared range [#16764](https://github.com/pnpm/pnpm/issues/16764).

- `pnpm audit --fix update` now updates only the dependencies whose locked version is vulnerable [#14928](https://github.com/pnpm/pnpm/issues/14928).

- `pnpm outdated` and `pnpm update --interactive` now apply `overrides` before they look up the latest version. Before, a dependency overridden to an npm alias was compared with the latest version of the package the override replaces [#16719](https://github.com/pnpm/pnpm/issues/16719).

#### Patched dependencies

- Patches saved with CRLF line endings now apply, including a patch that creates or deletes a file. pnpm rejected the git headers of such a patch with `ERR_PNPM_INVALID_PATCH` and the message `invalid file mode: 100644` [#16641](https://github.com/pnpm/pnpm/issues/16641).

- A patch that changes a file's mode, such as adding or removing the executable bit, now applies the new mode on Unix [pnpm/tasks#110](https://github.com/pnpm/tasks/issues/110).

#### Injected dependencies and deploy

- With `sharedWorkspaceLockfile: false`, an injected workspace package installed in the same run as its dependent now holds only the files its `files` field selects. The copy also held other files of the project, such as `tsconfig.json` [#16683](https://github.com/pnpm/pnpm/issues/16683).

- A script listed in `syncInjectedDepsAfterScripts` no longer fails when it rewrites `package.json` while pnpm is copying its edits into the injected copies. The sync after the script now replaces a half-copied manifest.

- `pnpm deploy` with a shared lockfile no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` for `settings.dedupeInjectedDeps` or `settings.dedupePeerDependents` when `lockfile.includeResolutionSettings` is enabled. The deployed lockfile now records both settings as `false`, the values the deploy installs with.

- `pnpm deploy` now writes the dependencies in the deployed `package.json` sorted by name. It also sorts the `allowBuilds` entries in the deployed `pnpm-workspace.yaml`. Repeated deploys of the same lockfile now produce identical files [#16687](https://github.com/pnpm/pnpm/issues/16687).

#### Packing and publishing

- `pnpm pack` and `pnpm publish` now match `.npmignore` and `.gitignore` rules the way npm does. A negation such as `!lib/**` or `!lib/**/!(*.map)` re-includes files under a directory that an earlier `*` rule excluded [#16743](https://github.com/pnpm/pnpm/issues/16743).

- `pnpm pack` and `pnpm publish` no longer always include root files that merely start with `README`, `LICENSE`, or `LICENCE`, such as `README_INTERNAL.md`. Only `README`, `LICENSE`, `LICENCE`, and `COPYING`, with or without an extension, ship regardless of `files` and `.npmignore`, as in npm [#16753](https://github.com/pnpm/pnpm/issues/16753).

- `pnpm publish --provenance=false` and `--no-provenance` now turn off provenance under trusted publishing, so a package can be published from a self-hosted runner. Setting `provenance: false` in `pnpm-workspace.yaml` does the same [#16721](https://github.com/pnpm/pnpm/issues/16721).

#### Speed and resource use

- `pnpm install` hardlinks files from a group-writable or world-writable store again. pnpm copied every file from such a store into `node_modules` [#16677](https://github.com/pnpm/pnpm/issues/16677).

- A repeat `pnpm install` no longer imports patched packages and packages with build scripts again when nothing changed. Their builds no longer run again either. This happened when `recursiveInstall` was `false` or the project had a `file:` dependency [#16705](https://github.com/pnpm/pnpm/issues/16705).

- Verifying the lockfile against `trustPolicy` and `minimumReleaseAge` uses less memory. pnpm no longer keeps every published version's manifest of each checked package in memory until the install ends [#16656](https://github.com/pnpm/pnpm/issues/16656).

- `pnpm rebuild <pkg>` and `pnpm rebuild --pending` no longer read the manifest of every installed package to find build scripts. Only the selected packages are inspected and built. On a large `node_modules` served lazily, such as over a network or FUSE mount, this turned a rebuild of a few packages into a fetch of every package.

- `pnpm rebuild` no longer removes and recreates `node_modules` when the settings recorded in `node_modules/.modules.yaml` differ from the current configuration. The rebuild runs the build scripts against the installed packages as they are.

- `pnpm store prune` now compacts the store's `index.db` after removing package entries, so the file shrinks again [#16717](https://github.com/pnpm/pnpm/issues/16717).

#### Registry commands

- `pnpm whoami`, `pnpm bugs`, `pnpm docs`, `pnpm repo`, `pnpm star`, `pnpm unstar`, and `pnpm stars` now honor the `--registry` option.

- `--registry` now takes precedence over a scope's configured registry for scoped packages in `pnpm access`, `pnpm view`, `pnpm repo`, `pnpm star`, `pnpm unstar`, `pnpm owner`, `pnpm deprecate`, `pnpm undeprecate`, `pnpm unpublish`, `pnpm dist-tag`, `pnpm team`, and `pnpm stage`. Without `--registry`, `pnpm access` now sends a scoped package or scope to the registry configured for that scope.

- Registry commands now keep the path of a registry URL such as `https://example.com/npm/` when they build request URLs. This applies to `pnpm access`, `pnpm owner`, `pnpm ping`, `pnpm search`, `pnpm star`, `pnpm stars`, `pnpm team`, `pnpm unstar`, and `pnpm whoami`.

- `pnpm view` now honors the network retry settings.

- `pnpm repo` now fetches the repository URLs of several packages in parallel.
