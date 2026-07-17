## 11.14.0

### Minor Changes

- `peerDependencies` now accept dependency specifiers that carry a scheme — a named-registry spec (`<registry>:<version>`), an `npm:` alias, or a `file:`/git/URL spec — instead of rejecting them with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` [#13095](https://github.com/pnpm/pnpm/issues/13095). Such a peer is matched against the semver range carried by the specifier (`work:5.x.x` is checked as `5.x.x`, `npm:bar@^5` as `^5`), or against `*` when it carries no version, while the original specifier still selects the package to auto-install. Bare `name@version` values, which are almost always a mistake, are still rejected.

- Added `pnpm doctor`, which diagnoses the pnpm installation and the environment it runs in: the versions and install method, whether the global bin directory is on `PATH`, whether the store and cache are writable, which link strategies (reflink, hardlink, symlink) the store's filesystem supports, registry connectivity, and an offline `file:` install that exercises the resolve/store/link path end to end. Each check reports how to fix what it finds, and the command exits non-zero when any check fails.

  Use `--offline` to skip the checks that need network access, `--json` for machine-readable output, and `--benchmark` to time the filesystem and install checks.

- Added support for executing multiple scripts matching a RegExp passed to `pnpm run` (e.g., `pnpm run "/^build:.*/"`), running matched scripts in deterministic lexicographical order. Restored the `--sequential` (`-s`) CLI option for `pnpm run`, which forces `workspaceConcurrency` to 1 so that matched scripts run sequentially one by one across and within packages.

### Patch Changes

- Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it [#13073](https://github.com/pnpm/pnpm/issues/13073). Reading a lockfile through a symlink is allowed again, and an install that leaves the lockfile unchanged no longer rewrites it, so `--frozen-lockfile` no longer needs to write at all. Writing a *changed* lockfile through a symlink is still refused, as that would redirect the write onto the symlink's target.

- Fixed frozen installs incorrectly treating equivalent Git dependency specifiers as a stale lockfile. See [#13039](https://github.com/pnpm/pnpm/issues/13039).

- `pnpm owner ls` now reports authentication and authorization failures (401/403) as dedicated errors that include the registry's response body, matching `pnpm owner add`/`rm`, instead of a generic `Failed to fetch owners` message.

- Recover from a metadata cache entry that disappears (concurrent cache cleanup, antivirus) after the registry has already answered the conditional request with `304 Not Modified`. The metadata is re-requested once without cache validators instead of failing the install with `ERR_PNPM_CACHE_MISSING_AFTER_304`.

- A project pinned to a broken pnpm release via `packageManager` or `devEngines.packageManager` now reports which release is broken and what to do about it, instead of failing inside the installer. `pnpm self-update` already refused these releases; the version switch does too.

- Prevent broken-lockfile errors from including snippets of the lockfile's contents.

- `pnpm self-update` now checks that the version it installed can run before making it the active pnpm. A release that installs but cannot execute is discarded with an error instead of replacing a working installation.

- Fixed an out-of-memory regression when workspace projects concurrently resolve a package with large registry metadata [pnpm/pnpm#13077](https://github.com/pnpm/pnpm/issues/13077).

- Fixed `pnpm update` rewriting exact version pins that use the `=` operator (for example `=3.5.1`) to a caret range (`^3.5.1`). Exact pins are now preserved and written back as the bare version. See pnpm/pnpm#12745.
