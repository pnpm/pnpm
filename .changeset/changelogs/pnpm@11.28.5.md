## 11.28.5

### Patch Changes

- `pnpm audit signatures` now verifies signatures against the integrity recorded in the lockfile. Packages without a recorded integrity cannot pass signature verification.

- The interactive `pnpm audit --fix` picker now shows each patched version with the `saveExact` and `savePrefix` style that the override is written with [pnpm/pnpm#13209](https://github.com/pnpm/pnpm/issues/13209).

- pnpm now prints config warnings, such as an unset environment variable in `.npmrc`, when loading the config fails.

- `pnpm publish` now rejects manifests and README files larger than 64 MiB in pre-built tarballs before reading them into memory.

- Large package downloads and large files inside gzip and bzip2 package archives now use bounded memory during installation. Package manifests and archive metadata larger than 64 MiB are rejected.

- pnpm now verifies locked config dependencies against their registry before installing them. Config dependencies must come from an npm registry. The lockfile can no longer replace the integrity of a config dependency pinned with `version+integrity`.

- A warning about a project's `devEngines` or `packageManager` pin is now printed to stderr. A command such as `pnpm cache path` or `pnpm list --json` keeps only its own output on stdout [#16584](https://github.com/pnpm/pnpm/issues/16584).

- `pnpm dlx` with `--package` but no command now reports a clear error: `'pnpm dlx' requires a command to run`. Previously it installed the package and then crashed trying to run an empty command.

- `pnpm list` now reports the correct package paths when `nodeLinker` is `hoisted` [#9593](https://github.com/pnpm/pnpm/issues/9593).

- Prevent bundled dependencies from traversing escaping directory symlinks or packaging files outside the package root [pnpm/tasks#83](https://github.com/pnpm/tasks/issues/83) [pnpm/tasks#93](https://github.com/pnpm/tasks/issues/93).

- `updateProjectManifestObject` now validates `saveType` against allowed dependency fields to prevent prototype pollution [pnpm/tasks#91](https://github.com/pnpm/tasks/issues/91).

- Fixed git dependency `#path:` subpath extraction traversing intermediate symlinks outside the repository root.

- Pointed the warning for a non-root `resolutions` field at the `overrides` field in `pnpm-workspace.yaml` [#11757](https://github.com/pnpm/pnpm/issues/11757).

- Ensure path-scoped registry prefix matching enforces a path segment boundary during unpublish [pnpm/tasks#94](https://github.com/pnpm/tasks/issues/94).

- `pnpm run` and `pnpm exec` now forward `--config.*` command-line flags to the install started by `verifyDepsBeforeRun` [pnpm/tasks#60](https://github.com/pnpm/tasks/issues/60).

- Dependency resolution reads cached registry metadata faster. The metadata cache moved to `<cache-dir>/v12/`, so the first install after upgrading downloads registry metadata again. A damaged cache entry is downloaded again, or reported as an error when `--offline` is set [#13512](https://github.com/pnpm/pnpm/pull/13512).

- `pnpm add` and `pnpm install` now keep the peer dependencies that `pnpm-lock.yaml` records for a package they did not update. A registry whose metadata disagrees with the package's `package.json`, for example by omitting `peerDependenciesMeta`, made `pnpm add` and `pnpm dedupe` write different lockfiles, so `pnpm dedupe --check` failed after `pnpm add` [#16615](https://github.com/pnpm/pnpm/issues/16615).

- `pnpm config get` and `pnpm config list` with `--global` or `--location=global` now show only the global configuration. Both flags included the project's `.npmrc` before. `--location=global` also included the project's `pnpm-workspace.yaml`. `pnpm config get --global` failed when the global bin directory was not in PATH [#16598](https://github.com/pnpm/pnpm/issues/16598).

- Lockfile verification now checks the tarballs inside a `variations` resolution against the registry. A `name@version` lockfile entry with an empty `variations` resolution is now rejected.

- `pnpm dlx` now inherits the `release` entry of `nodeDownloadMirrors` from the workspace configuration when downloading Node.js runtimes [#11281](https://github.com/pnpm/pnpm/issues/11281). Other channels are not inherited: Node signs `SHASUMS256.txt` for `release` only, so a workspace-supplied `rc`/`nightly` mirror would provide both the archive and its checksum for a runtime that `dlx` executes. Mirrors the user configured globally for those channels continue to apply.

- The warnings about ignored project `.npmrc` registry and auth settings no longer print the username and password of a URL-scoped key such as `//user:password@registry.example.com/:_authToken`.

- When a dependency moves an exact dependency of its own to an older version, a peer dependency that pnpm installed automatically now moves with it. Before, `pnpm install` and `pnpm dedupe` kept the newer locked version of the peer, so the lockfile held two copies of it, for example two copies of `vue` [pnpm/tasks#61](https://github.com/pnpm/tasks/issues/61).

- `pnpm deploy` with `deployAllFiles` now rejects symlinks that point outside the package directory. Local package installs with this setting apply the same check.

- `pnpm licenses` now removes terminal control characters from package metadata in table output.

- `pnpm setup` now names the shell config file even if it is already up to date [#16608](https://github.com/pnpm/pnpm/issues/16608).

- `pnpm setup` now puts `$PNPM_HOME/bin` first on `PATH` in login shells that inherited it further down, such as the VS Code terminal on macOS. Before, another `node` took precedence over the one installed by `pnpm runtime set node -g`. Run `pnpm setup` again to update the block in your shell config [#16635](https://github.com/pnpm/pnpm/issues/16635).

- Validate PAX header record lengths and use 64-bit modulo arithmetic for tar block padding [pnpm/tasks#78](https://github.com/pnpm/tasks/issues/78) [pnpm/tasks#79](https://github.com/pnpm/tasks/issues/79).

- Fixed two URL or local path dependencies sharing a virtual store directory when one URL had `+`, `#`, `:`, or `?` where the other had `/`. Such dependencies, including git dependencies pinned with `#`, now get a hash suffix on their directory name.

- `pnpm unpublish <pkg>@<version>` now deletes the tarball under the registry's path when the registry is served under one, such as Gitea's npm registry. It used to send the delete to the host root and report success without removing the version [#16568](https://github.com/pnpm/pnpm/issues/16568).

- `pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a dependency uses a specifier with a protocol pnpm does not support, such as Yarn's `patch:`. pnpm linked such a dependency to a directory that does not exist [#16590](https://github.com/pnpm/pnpm/issues/16590).

- pnpm now rejects a git dependency whose lockfile repository is empty, begins with `-`, or contains a null byte. Git can no longer read such a value as a command-line option [pnpm/tasks#84](https://github.com/pnpm/tasks/issues/84).

- Validate the httpProxy and httpsProxy settings as strings when configured in pnpm-workspace.yaml or global configuration.
