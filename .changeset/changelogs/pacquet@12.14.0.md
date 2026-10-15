## 12.14.0

This release adds the `sideEffectsCache.exclude` setting, stops `pnpm install` from waiting on the daily update check, and keeps pnpm from aborting with a core dump when the reader of its output exits.

### Minor Changes

- Added the `sideEffectsCache.exclude` setting for packages whose build output depends on the environment, such as native addons that read `JAVA_HOME`. pnpm builds the listed packages in every project and never restores or saves their builds through the side-effects cache, locally or on a `remoteCache` server. Under the global virtual store, each project gets its own copy of these packages and of the packages that depend on them. Entries use the same `name`, `@scope/*`, and `name@version` patterns as `minimumReleaseAgeExclude` [#5271](https://github.com/pnpm/pnpm/issues/5271).

- `pnpm login` now prints the `npm-notice` response header during web-based login. Registries use it to show the verification code that completes the sign-in in the browser.

### Patch Changes

#### Installing packages

- `pnpm install` and `pnpm add` no longer wait for the daily update check. If the command finishes first, pnpm skips the update notice and checks again next time. On a registry that accepted the connection but never answered, an install that had nothing to do used to take minutes [#16839](https://github.com/pnpm/pnpm/issues/16839).

- `pnpm install`, `pnpm add` and `pnpm update` now reuse `pnpm-lock.yaml` when the workspace configures a catalog entry that no dependency references, such as an unused entry or one whose package an `overrides` entry replaces. Before, every resolution in such a workspace resolved all dependencies again from the registry [#16923](https://github.com/pnpm/pnpm/issues/16923).

- `pnpm install` now looks up a git dependency's branch or tag once per install, even when many workspace projects depend on it. With `linkWorkspacePackages` enabled, it used to run `git ls-remote` once per project [#16899](https://github.com/pnpm/pnpm/issues/16899).

- A filtered install, such as `pnpm install --filter <pkg>`, now keeps the `pnpm-lock.yaml` entries of workspace projects that are missing from disk, as in a sparse checkout. Their importers, packages, and catalog entries stay even when the filter matches every project on disk [#16931](https://github.com/pnpm/pnpm/issues/16931).

- pnpm now decides whether a dependency that a package also declares as an optional peer is linked as a dependency or as a peer from the shallowest path that reaches the package. When an existing lockfile records such a dependency as a regular dependency and a newly added dependency now provides the peer, pnpm records it as a peer, as a fresh install does. The lockfile entry of such a package can change on the next install [#16901](https://github.com/pnpm/pnpm/issues/16901).

- An install that adds nothing to the store no longer changes the modification time of the store directory. CI caches that check modification times, such as GitLab's, no longer re-upload an unchanged store [#16929](https://github.com/pnpm/pnpm/issues/16929).

#### Running commands and scripts

- pnpm no longer aborts with a core dump when the reader of its output has exited, as in `pnpm list | head -1` or `pnpm start 2>&1 | head -1`. The command finishes with its usual exit code, and a failing script's exit code is kept. A write to stdout that fails for another reason, such as a full disk, ends the command with exit code 1 [#16904](https://github.com/pnpm/pnpm/issues/16904) [pnpm/tasks#132](https://github.com/pnpm/tasks/issues/132).

- On Windows, `pnpm exec`, `pnpm run`, and commands such as `pnpm nx` now return when the command they run exits, even if it left a detached process running and pnpm's output is redirected or piped. Previously a shell reading that output waited until the detached process, such as the Nx daemon, exited [#16930](https://github.com/pnpm/pnpm/issues/16930).

- Commands that do not write the project manifest now leave a missing `package.json` absent. This covers queries such as `pnpm why` and `pnpm outdated`, `pnpm fetch`, `pnpm dedupe --check`, and commands that exit with an error, such as `pnpm rt ls`. These commands used to create one from the initialization template. `pnpm sbom` run from a lockfile alone now reports the root component as `unknown` with version `0.0.0` [#16907](https://github.com/pnpm/pnpm/issues/16907).

- `pnpm run` now reports a failed script as a JSON record under `--reporter=ndjson`. It printed a plain `[ELIFECYCLE]` line to stderr.

- `pnpm run` no longer prints `[ELIFECYCLE] Command failed with exit code N.` to stderr when a script fails under `--reporter=silent`, `--silent`, or `--loglevel=silent`.

- `pnpm pipeline --dry-run` no longer writes a run report when no projects are selected. With `--json` it prints `{"tasks": []}` [pnpm/tasks#126](https://github.com/pnpm/tasks/issues/126).

#### Packing and publishing

- `pnpm pack` and `pnpm publish` now ship the whole gitignored directory that `main`, `browser`, or `bin` points into, as npm does. Previously only the `main` and `bin` files themselves were packed, and a gitignored `browser` file was left out [#16908](https://github.com/pnpm/pnpm/issues/16908).

- `pnpm pack` and `pnpm publish` now honor a `.npmignore` or `.gitignore` inside a subdirectory when `package.json` has a `files` field. The package root's ignore files and those inherited from the workspace are still skipped [#16909](https://github.com/pnpm/pnpm/issues/16909).

#### Setting up pnpm and toolchains

- `--registry`, `--https-proxy`, `--http-proxy`, and `--no-proxy` now apply when pnpm resolves and downloads the pnpm version a project pins in `packageManager`. Before, these lookups went to `https://registry.npmjs.org/`, which a firewall that only allows a private registry blocks [#16911](https://github.com/pnpm/pnpm/discussions/16911).

- On Windows, `pnpm install` now installs the Rust toolchain for rustup's default host, so a machine set up with `rustup set default-host x86_64-pc-windows-gnu` gets the MinGW toolchain. Without a default host, pnpm installs the MSVC toolchain for the machine's architecture, as rustup does. This includes a pnpm built for MinGW, such as the MSYS2 package, which has installed the MinGW toolchain since 12.13.0. An x64 pnpm on ARM64 Windows now installs the ARM64 toolchain [#16854](https://github.com/pnpm/pnpm/issues/16854).
