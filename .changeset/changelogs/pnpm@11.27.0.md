## 11.27.0

### Minor Changes

- `nodeDownloadMirrors` can now be set in the global config file (`config.yaml`) and through the `PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS` environment variable, so a Node.js download mirror can be configured once for a machine instead of in every workspace [#12124](https://github.com/pnpm/pnpm/issues/12124), [#13611](https://github.com/pnpm/pnpm/issues/13611).

  ```sh
  PNPM_CONFIG_NODE_DOWNLOAD_MIRRORS='{"release":"https://npmmirror.com/mirrors/node/"}'
  ```

- Added a new setting `trustPolicyExcludePrune` (default: `false`). When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `trustPolicyExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

### Patch Changes

- pnpm now reads the `packageManager`, `devEngines.packageManager` and runtime pins from the workspace root's `package.json` when `lockfileDir` is set. A project that moved its lockfile lost the pins it declared there [#14633](https://github.com/pnpm/pnpm/issues/14633).

- Fixed `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` mutating global bins or install directories after only partially reading an installed package group. If any declared package manifest is missing, malformed, or unreadable, pnpm now fails before activation or removal and leaves the existing global installation intact [pnpm/pnpm#13796](https://github.com/pnpm/pnpm/issues/13796).

- `fetch-timeout` now limits how long a request may make no progress. The timer restarts on every chunk that arrives. A large download over a slow connection is no longer aborted while data is still coming in. A connection that stops delivering data still fails after `fetch-timeout` [#14604](https://github.com/pnpm/pnpm/issues/14604).

- `pnpm peers check` no longer reports a peer dependency declared as `workspace:^`, `workspace:~`, or a bare `workspace:` as unmet. pnpm reported these as unmet whatever version the linked workspace project supplied [#14770](https://github.com/pnpm/pnpm/issues/14770).

- A `readPackage` hook that edits its argument in place no longer changes what a later install in the same command resolves. A `deprecated` notice read from the lockfile no longer carries over to another install either [#13988](https://github.com/pnpm/pnpm/issues/13988).

- `pnpm install` now auto-installs missing transitive peers when workspace projects share a dependency at different depths. This also removes incomplete duplicate peer contexts from the lockfile. Fixes [pnpm/pnpm#14840](https://github.com/pnpm/pnpm/issues/14840).

- GitHub Actions updates now stop if an action reference changes while its versions are being resolved. Unrelated workflow edits are preserved.

  GitHub Actions homepage links no longer expose server credentials. GitHub server URLs now require HTTPS, with HTTP allowed only for loopback hosts.

- `pnpm licenses list` now reports the runtime downloaded through `devEngines.runtime` with `onFail: "download"`. The command previously failed with `ERR_PNPM_UNSUPPORTED_PACKAGE_TYPE` [#14172](https://github.com/pnpm/pnpm/issues/14172).

- pnpm no longer creates a project `pnpm-lock.yaml` when `devEngines.packageManager.onFail` is `download` and lockfile writing is turned off with `lockfile: false` or `--no-lockfile`. pnpm still switches to the pinned version [#14728](https://github.com/pnpm/pnpm/issues/14728).

- A `registry` or `@scope:registry` set in an `.npmrc` now wins over the registry a `pnpm login` credential stored in the global `config.yaml` points at. Previously, after logging in to one registry, installs in a project whose `.npmrc` named a private registry went to the logged-in registry instead. They now go to the registry the `.npmrc` names [#14614](https://github.com/pnpm/pnpm/issues/14614).

- A patch that gives a dependency a `preinstall`, `install`, or `postinstall` script, or a `binding.gyp`, now runs that build. pnpm asks for build approval first, so the package is listed under "Ignored build scripts" until it is allowed to build. pnpm 12 ran nothing, and pnpm 11 ran it without asking [#14648](https://github.com/pnpm/pnpm/issues/14648).

- Registries that share a host but differ by URL path — one JFrog Artifactory, Nexus, AWS CodeArtifact or GitLab Packages instance serving several repositories — now get a metadata cache directory each. Previously they shared one, so resolving a package from one of them could answer with another's versions, integrity hashes and tarball URLs and fail with `ERR_PNPM_TARBALL_URL_MISMATCH` [#13558](https://github.com/pnpm/pnpm/issues/13558).

  The URL scheme is part of the cache directory name too, so an `http` registry can no longer hand its metadata — which can be rewritten in transit — to a resolution configured for `https` at the same host.

  The first install after upgrading refetches registry metadata once. The package store is untouched.

  `pnpm cache view` now labels each entry with the full registry URL. It printed `registry.npmjs.org` before and prints `https://registry.npmjs.org/` now.

  `pnpm cache list-registries` and `pnpm cache list` print the new directory names. Scripts that parse either command need updating.

- Updated the embedded Node.js release keys to the current canonical `nodejs/release-keys` list.

- `pnpm sbom` now omits package author fields when the manifest author name is empty or contains only whitespace [pnpm/pnpm#14685](https://github.com/pnpm/pnpm/issues/14685). In a filtered or split workspace run, only a project with no `author` field inherits the workspace root's author.

- `pnpm sbom --sbom-format spdx` now writes `creationInfo.created` with whole seconds, such as `2026-09-08T10:38:21Z`. The timestamp carried fractional seconds, which strict SPDX consumers rejected [#14684](https://github.com/pnpm/pnpm/issues/14684).

- Windows filesystem operations now retry permission errors for up to one second. Permanent permission errors previously delayed failure by a minute. Sharing and lock violations retain their one-minute retry budget [pnpm/pnpm#14682](https://github.com/pnpm/pnpm/issues/14682).

- pnpm now writes `node_modules/.package-map.json` only when `nodeExperimentalPackageMap` is enabled. Nothing reads the file without that setting. An install that stops writing the map removes the one a previous install left.

- pnpm now unpacks a downloaded runtime archive into a randomly named directory inside the store. It previously used a predictable path, where another user of a shared store could plant a symlink and redirect the write outside the store ([GHSA-vwc7-r8mq-g2x9](https://github.com/advisories/GHSA-vwc7-r8mq-g2x9)).
