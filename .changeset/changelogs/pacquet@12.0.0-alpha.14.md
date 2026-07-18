## 12.0.0-alpha.14

### Minor Changes

- Added the `owner` command (aliased as `owners`) for managing package owners on the registry, with the `ls` (default), `add`, and `rm` subcommands and support for `--registry` and `--otp`.

- `peerDependencies` now accept dependency specifiers that carry a scheme — a named-registry spec (`<registry>:<version>`), an `npm:` alias, or a `file:`/git/URL spec — instead of rejecting them with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` [#13095](https://github.com/pnpm/pnpm/issues/13095). Such a peer is matched against the semver range carried by the specifier (`work:5.x.x` is checked as `5.x.x`, `npm:bar@^5` as `^5`), or against `*` when it carries no version, while the original specifier still selects the package to auto-install. Bare `name@version` values, which are almost always a mistake, are still rejected.

- Added `pnpm doctor`, which diagnoses the pnpm installation and the environment it runs in: the versions and install method, whether the global bin directory is on `PATH`, whether the store and cache are writable, which link strategies (reflink, hardlink, symlink) the store's filesystem supports, registry connectivity, and an offline `file:` install that exercises the resolve/store/link path end to end. Each check reports how to fix what it finds, and the command exits non-zero when any check fails.

  Use `--offline` to skip the checks that need network access, `--json` for machine-readable output, and `--benchmark` to time the filesystem and install checks.

- Added support for executing multiple scripts matching a RegExp passed to `pnpm run` (e.g., `pnpm run "/^build:.*/"`), running matched scripts in deterministic lexicographical order. Restored the `--sequential` (`-s`) CLI option for `pnpm run`, which forces `workspaceConcurrency` to 1 so that matched scripts run sequentially one by one across and within packages.

### Patch Changes

- Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it [#13073](https://github.com/pnpm/pnpm/issues/13073). Reading a lockfile through a symlink is allowed again, and an install that leaves the lockfile unchanged no longer rewrites it, so `--frozen-lockfile` no longer needs to write at all. Writing a *changed* lockfile through a symlink is still refused, as that would redirect the write onto the symlink's target.

- Limit registry-provided gzip preallocation hints to 64 MiB so oversized `dist.unpackedSize` values cannot trigger excessive eager allocation.

- Fixed frozen installs incorrectly treating equivalent Git dependency specifiers as a stale lockfile. See [#13039](https://github.com/pnpm/pnpm/issues/13039).

- Fixed `pnpm install` aborting with a panic when a project depends on a git-hosted package [#13040](https://github.com/pnpm/pnpm/issues/13040).

- `node_modules/.modules.yaml` now records `packageManager` as `pnpm@<release version>` (for example `pnpm@12.0.0-alpha.13`), matching `pnpm --version` and the TypeScript CLI. It previously recorded the internal crate name and crate version, `pacquet@0.0.1`.

- Fixed `pacquet add` to accept and install multiple package selectors in one operation.

- Improved `pnpm add` performance for multiple package selectors by resolving them concurrently [pnpm/pnpm#13089](https://github.com/pnpm/pnpm/issues/13089).

- Every error code is now an `ERR_PNPM_*` code, matching the codes pnpm has always used. Errors previously reported internal Rust-crate codes such as `pacquet_package_manager::outdated_lockfile` or unprefixed codes such as `GIT_CHECKOUT_FAILED`; these are now `ERR_PNPM_OUTDATED_LOCKFILE` and `ERR_PNPM_GIT_CHECKOUT_FAILED`. Where pnpm defines a code for the same error, pnpm's exact code is used. Scripts and CI that match on the old codes need updating.

- Fixed fresh isolated installs to enforce incompatible required dependency engines when `engineStrict` is enabled.

- `pnpm add <pkg>` (without a version) and `pnpm update --latest` now resolve the `latest` dist-tag through the `minimumReleaseAge`-aware picker, pinning the newest version that satisfies the cutoff instead of writing a range the follow-up install rejects. An invalid `minimumReleaseAgeExclude` value reported by these commands now carries the same `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` error code the install reports. See pnpm/pnpm#11165.

- Fixed duplicate package statistics output during installs in non-interactive terminals.

- Prompt before installing packages that do not meet a strict `minimumReleaseAge`, persist approved versions to `minimumReleaseAgeExclude`, and keep progress output from overwriting the prompt.

- Error messages and `--help` text now refer to the CLI as `pnpm` instead of the internal `pacquet` name. Several messages previously suggested commands like `pacquet install --frozen-lockfile`, which is not a command users can run, and `pnpm add --help` documented the virtual store directory default as `node_modules/.pacquet` rather than the actual `node_modules/.pnpm`.

- Fixed installs to detect manifest changes in workspace members and reject stale lockfiles when using `--frozen-lockfile` [pnpm/pnpm#13080](https://github.com/pnpm/pnpm/issues/13080).

- Recover from a metadata cache entry that disappears (concurrent cache cleanup, antivirus) after the registry has already answered the conditional request with `304 Not Modified`. The metadata is re-requested once without cache validators instead of failing the install with `ERR_PNPM_CACHE_MISSING_AFTER_304`.

- A project pinned to a broken pnpm release via `packageManager` or `devEngines.packageManager` now reports which release is broken and what to do about it, instead of failing inside the installer. `pnpm self-update` already refused these releases; the version switch does too.

- Prevent broken-lockfile errors from including snippets of the lockfile's contents.

- `pnpm self-update` now checks that the version it installed can run before making it the active pnpm. A release that installs but cannot execute is discarded with an error instead of replacing a working installation.

- Fixed `pnpm update` rewriting exact version pins that use the `=` operator (for example `=3.5.1`) to a caret range (`^3.5.1`). Exact pins are now preserved and written back as the bare version. See pnpm/pnpm#12745.
