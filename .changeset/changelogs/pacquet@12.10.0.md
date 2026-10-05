## 12.10.0

### Minor Changes

- Added experimental `nodeLinker: { type: loaded }` installation. Compatible dependencies load directly from the content-addressable store through an automatically registered Node.js loader. `nodeLinker.excluded` selects packages and their dependency trees to install in the global virtual store.

- `lockfile.includeResolutionSettings: true` makes `pnpm-lock.yaml` record `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`. Installs then treat a lockfile that records other values as outdated. A lockfile that records `autoDedupe` is reused by later installs on any machine, so `pnpm run` after `pnpm install --frozen-lockfile` no longer starts another install [#16583](https://github.com/pnpm/pnpm/issues/16583).

### Patch Changes

- `pnpm audit signatures` now verifies signatures against the integrity recorded in the lockfile. Packages without a recorded integrity cannot pass signature verification.

- The interactive `pnpm audit --fix` picker now shows each patched version with the `saveExact` and `savePrefix` style that the override is written with [pnpm/pnpm#13209](https://github.com/pnpm/pnpm/issues/13209).

- `pnpm install` and `pnpm publish` now reject archive metadata larger than 64 MiB before reading it into memory. Publishing a pre-built tarball also rejects manifests and README files larger than 64 MiB.

- `pnpm cache prune` now also removes the registry metadata cache that older pnpm versions wrote under `<cache-dir>/v11/` [#13512](https://github.com/pnpm/pnpm/pull/13512).

- On Windows, `pnpm self-update` no longer runs the update a second time when it replaces a `pnpm.cmd` linked by pnpm 12.8 or older. cmd.exe read on in the replaced `pnpm.cmd`, printed an error about a command that is not recognized, and ran the new pnpm once more [#16573](https://github.com/pnpm/pnpm/issues/16573).

- pnpm now verifies locked config dependencies against their registry before installing them. Config dependencies must come from an npm registry. The lockfile can no longer replace the integrity of a config dependency pinned with `version+integrity`.

- pnpm now prints config warnings, such as an unset environment variable in `.npmrc`, when loading the config fails.

- Allow custom resolutions under name@version keys during lockfile verification.

- `pnpm dedupe` now reads registry metadata for a dependency pinned to an exact version, as `pnpm install` does. If the registry metadata disagreed with the package's `package.json`, the lockfile it wrote depended on whether `minimumReleaseAge` was set [#16615](https://github.com/pnpm/pnpm/issues/16615).

- A warning about a project's `devEngines` or `packageManager` pin is now printed to stderr. A command such as `pnpm cache path` or `pnpm list --json` keeps only its own output on stdout [#16584](https://github.com/pnpm/pnpm/issues/16584).

- Empty `nodeOptions` values from command-line flags and environment variables now override lower-priority settings. Scripts retain `NODE_OPTIONS` from the parent environment or `extraEnv` when `nodeOptions` is empty.

- Commands in a project that pins a different pnpm version start about 13 ms faster on macOS. pnpm now runs the pinned version's binary directly, without the shell script in front of it [pnpm/tasks#66](https://github.com/pnpm/tasks/issues/66).

- pnpm now reads the `failIfNoMatch` setting from `pnpm-workspace.yaml`, so a filter that matches no workspace project exits with code 1 when the setting is `true`. The new `--no-fail-if-no-match` flag turns the setting off for one command [#16577](https://github.com/pnpm/pnpm/issues/16577).

- Resolution errors now name the failing dependency and its parent packages. Fatal errors appear as structured error records with their error codes when using `--reporter=ndjson`.

- `pnpm list` now reports the correct package paths when `nodeLinker` is `hoisted` [#9593](https://github.com/pnpm/pnpm/issues/9593).

- `pnpm install --fix-lockfile` repairs a lockfile whose importer references a package that has no snapshot entry, as left by a badly merged lockfile. It failed with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` since 12.8.0 [#16618](https://github.com/pnpm/pnpm/issues/16618).

- Fixed `pnpm install` failing with `ERR_PNPM_CMD_SHIM_RESOLVE_PATH` when an executable's parent directory contains a dangling symlink.

- Ensure path-scoped registry prefix matching enforces a path segment boundary during unpublish [pnpm/tasks#94](https://github.com/pnpm/tasks/issues/94).

- `pnpm run` and `pnpm exec` now forward `--config.*` command-line flags to the install started by `verifyDepsBeforeRun` [pnpm/tasks#60](https://github.com/pnpm/tasks/issues/60).

- `pnpm install` now prevents dependency versions with path traversal from writing files outside the global virtual store.

- `pnpm config get --global` and `pnpm config list --global` now show only the global configuration, also when run inside a project. Settings from the project's `pnpm-workspace.yaml` and `.npmrc` were included before. The same applies to `--location=global` [#16598](https://github.com/pnpm/pnpm/issues/16598).

- Dependency resolution reads cached registry metadata faster. The metadata cache moved to `<cache-dir>/v12/`, so the first install after upgrading downloads registry metadata again. A damaged cache entry is downloaded again, or reported as an error when `--offline` is set [#13512](https://github.com/pnpm/pnpm/pull/13512).

- The error for an invalid git repository in the lockfile now has the code `ERR_PNPM_INVALID_GIT_REPOSITORY`. Its message now lists every rejected form of the value.

- Lockfile verification now checks the tarballs inside a `variations` resolution against the registry. A `name@version` lockfile entry with an empty `variations` resolution is now rejected.

- Package metadata requests no longer wait behind queued tarball downloads when `maxSockets` or a proxy limits the connections to a registry. Large installs resolve faster and print fewer `Request took` warnings.

- pnpm 11 releases older than 11.28.4 can run pnpm 12 again when the `packageManager` field pins it. Since 12.9.0 they failed with `SyntaxError: Invalid or unexpected token` [#16594](https://github.com/pnpm/pnpm/issues/16594).

- The warning about an ignored project `.npmrc` registry setting no longer prints the username and password of a URL-scoped key such as `//user:password@registry.example.com/${PATH}/:_authToken`.

- `pnpm install` now fails with `ERR_PNPM_PACKAGE_MANIFEST_INVALID_ATTRIBUTE` when a project declares a dependency whose specifier is not a string, such as `"is-positive": 42`. Before, the dependency was silently left out of the lockfile. A `readPackage` hook can still correct the specifier.

- Sped up installs in large workspaces on macOS when the dependency links already exist. pnpm now keeps a link that already points at the right package without trying to create it first. Relinking the direct dependencies of 1,000 workspace projects took 45 ms, down from 116 ms [pnpm/tasks#65](https://github.com/pnpm/tasks/issues/65).

- When a dependency moves an exact dependency of its own to an older version, a peer dependency that pnpm installed automatically now moves with it. Before, `pnpm install` and `pnpm dedupe` kept the newer locked version of the peer, so the lockfile held two copies of it, for example two copies of `vue` [pnpm/tasks#61](https://github.com/pnpm/tasks/issues/61).

- `pnpm run` no longer prints `[ELIFECYCLE] Command failed ...` after Ctrl+C ends the script. pnpm still exits the way the script's shell did: on Windows with the shell's exit code (`cmd` reports `-1073741510`, PowerShell `1`), on Unix by re-raising `SIGINT` [#16579](https://github.com/pnpm/pnpm/issues/16579).

- `pnpm run "/<regex>/"` now accepts JavaScript regular expression syntax such as lookahead and lookbehind. A selector like `"/^hello:(?!b).*$/"` failed with `ERR_PNPM_NO_SCRIPT` [#16604](https://github.com/pnpm/pnpm/issues/16604).

- `pnpm runtime --help` and `pnpm help runtime` now name the `set` subcommand and the runtimes it accepts [#16580](https://github.com/pnpm/pnpm/issues/16580).

- `pnpm setup` now names the shell config file even if it is already up to date [#16608](https://github.com/pnpm/pnpm/issues/16608).

- `pnpm setup` now puts `$PNPM_HOME/bin` first on `PATH` in login shells that inherited it further down, such as the VS Code terminal on macOS. Before, another `node` took precedence over the one installed by `pnpm runtime set node -g`. Run `pnpm setup` again to update the block in your shell config [#16635](https://github.com/pnpm/pnpm/issues/16635).

- The pnpm binary for arm64 Linux is about 1 MB smaller.

- The pnpm binary is about 0.9 MB smaller.

- Fixed two URL or local path dependencies sharing a virtual store directory when one URL had `+`, `#`, `:`, or `?` where the other had `/`. Such dependencies, including git dependencies pinned with `#`, now get a hash suffix on their directory name.

- `pnpm unpublish <pkg>@<version>` now deletes the tarball under the registry's path when the registry is served under one, such as Gitea's npm registry. It used to send the delete to the host root and report success without removing the version [#16568](https://github.com/pnpm/pnpm/issues/16568).

- `pnpm update --latest` now applies the `savePrefix` setting when it rewrites a dependency whose range has no operator of its own, such as `<2.0.0`.

- `pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a dependency uses a specifier with a protocol pnpm does not support, such as Yarn's `patch:`. On Windows, such a specifier failed with `os error 123`. On other platforms, pnpm linked it to a directory that does not exist. Reading a `package.json` that fails now names the file [#16590](https://github.com/pnpm/pnpm/issues/16590).

- On Windows, a process started by `pnpm run` or `pnpm exec` can again start a child with `CREATE_BREAKAWAY_FROM_JOB`. That child keeps running after pnpm exits, even if the command fails [#16628](https://github.com/pnpm/pnpm/issues/16628).
