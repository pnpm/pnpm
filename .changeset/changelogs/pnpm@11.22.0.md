## 11.22.0

### Minor Changes

- Added `pnpm cache path`, which prints the directory pnpm uses for its metadata cache. CI setups can use it to cache that directory — including the lockfile verification log, which lets a job skip re-checking an unchanged lockfile against the configured supply-chain policies.

- `--config.config-dir` no longer reaches the config through a project's `pnpm-workspace.yaml`, and neither do the `--config.` spellings of the other settings a project manifest may no longer contribute (`--config.pnpm-home-dir`, `--config.workspace-dir`, `--config.global-pkg-dir`, `--config.root-project-manifest-dir`). None of them was ever a supported way to set those directories: pnpm resolves them from the environment, and these flags took effect only because the project-manifest merge re-applied the command line afterwards. The dedicated flags, such as `--dir` and `--global-dir`, are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).

- `pnpm config set` refuses to write a setting to a project's `pnpm-workspace.yaml` that pnpm does not read from there, rather than leaving a key in the file that does nothing. Those settings are `configDir`, `pnpmHomeDir`, `stateDir` and the others that name machine-level state. The command fails with `ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING`, naming where the setting does belong when it belongs somewhere. `pnpm config delete` still clears one that a file already carries, in whichever spelling it uses [#13629](https://github.com/pnpm/pnpm/issues/13629).

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

- A project's `pnpm-workspace.yaml` can no longer choose where pnpm keeps its credentials, its own installation, or the registry it downloads its next version from. One of those settings is `configDir`, which decided where `pnpm login` writes the granted token. `bin`, `dir`, `globalBinDir`, `globalDir`, `npmrcAuthFile`, `pnpmHomeDir`, `stateDir`, `userconfig` and `workspaceDir` are ignored there now too, and pnpm warns about the ones it finds. `cacheDir` and `storeDir` are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).

- Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster: the per-version release metadata is cached in the pnpm cache directory after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the noticeable delay on the first `node` invocation in a project pinning an already-downloaded runtime [#13899](https://github.com/pnpm/pnpm/issues/13899).

### Patch Changes

- Fixed intermittent `ERR_PNPM_ENOENT` and `ERR_PNPM_ENOTEMPTY` errors while renaming `_tmp_*` directories during installation with `nodeLinker: hoisted`, in workspaces that also use `patchedDependencies`.

- `pnpm add` no longer re-resolves the dependency graph when `pnpm-lock.yaml` already holds a version satisfying the request — promoting a transitive dependency to a direct one, or adding to a second workspace package what a first one already depends on, now only saves the dependency in `package.json` and records its importer entry. A satisfying locked version is necessary but not sufficient: the install still falls back to a full resolution for a dist tag, an alias, a `workspace:`/`catalog:`/git/tarball specifier, `--save-peer`, an overridden package, a `catalogMode` other than `manual`, and — under `resolutionMode: time-based` or `lowest-direct`, which resolve a direct dependency to the low end of its range — a range several locked versions satisfy.

- Global installs now switch over atomically. The command shims in the global bin directory point at a stable per-package link rather than at the directory a particular install produced, so `pnpm add -g` and `pnpm update -g` activate a new version by moving that one link instead of rewriting every shim. A command can no longer be missing from `PATH` while an install is in progress, and a failed install leaves the previous version in place.

- `pnpm audit --fix` and `pnpm audit --fix update` no longer add `minimumReleaseAgeExclude` entries for patched versions that were published before the `minimumReleaseAge` cutoff. The publish time of each minimum patched version is now checked against the registry metadata, and only versions young enough to be blocked by the age gate get an exclusion entry [#11563](https://github.com/pnpm/pnpm/issues/11563).

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under a non-manual `catalogMode` now move the catalog entry's resolution to the requested version. Previously, when the catalog entry was a range that covered the requested version but resolved to a different one, the request was dropped silently: nothing was installed, nothing was written, and no error was raised.

- A project that wasn't part of an install that moved a catalog entry now follows the entry the next time it is installed. It used to keep the version the entry resolved to before — a version the entry no longer allowed — and no later install corrected it, so one catalog entry ended up resolved to two versions.

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under `catalogMode: strict` no longer fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` when the catalog entry is a range that the wanted version satisfies. The dependency keeps using the catalog; only a version that really falls outside the catalog's range is rejected [#13715](https://github.com/pnpm/pnpm/issues/13715).

- A changed `catalogs` or `pnpm.overrides` block no longer has to be the only change for `pnpm install` to update the lockfile in place. Editing an override while also removing a dependency, or changing a catalog entry in the same commit as a range bump, is now absorbed in one pass instead of re-resolving the whole dependency graph [#13799](https://github.com/pnpm/pnpm/issues/13799).

  Fixed the lockfile an in-place override update wrote when the overridden package was also a catalog entry: the entry kept the version it had before the override moved the package. The same could happen in reverse, when a catalog entry moved a package an override pins. Both cases now re-resolve instead.

- `pnpm install` now updates the lockfile in place even when several kinds of changes happened since the last install — for example a removed dependency together with a widened `ignoredOptionalDependencies` list, or a dependency edit alongside a patch or settings change. Previously any combination of changes forced a full re-resolution [#13763](https://github.com/pnpm/pnpm/issues/13763).

- `pnpm deploy` injects workspace dependencies again, so the deploy directory is self-contained instead of symlinking back into the source workspace [#13754](https://github.com/pnpm/pnpm/issues/13754). Enabling `injectWorkspacePackages` with `dedupeInjectedDeps` disabled now also rewrites already-linked workspace dependencies to injected copies.

- `pnpm deploy --no-optional` no longer writes a lockfile whose snapshots reference optional dependencies that the deploy excluded.

- Removing the last dependency that references a catalog entry via the fast lockfile update no longer leaves the stale catalog entry in `pnpm-lock.yaml`.

- A git dependency whose clone (or shallow fetch) fails now reports which package it belongs to, under the `ERR_PNPM_GIT_FETCH_FAILED` code, with credentials in the repository URL redacted. When the lockfile records an SSH remote, the error also explains that fetching it needs an SSH key for that host, and that a lockfile entry written before pnpm v11.21 can be re-recorded over HTTPS with `pnpm update <package>` [#13743](https://github.com/pnpm/pnpm/issues/13743).

- An `integrity` recorded on a git dependency's resolution (`resolution: {type: git, repo, commit, integrity: sha512-…}`) is no longer treated as a checksum. pnpm never verifies a git checkout against such a hash — the commit pins the content — so it is now dropped when the lockfile is rewritten, and `pnpm sbom` no longer republishes it as a CycloneDX/SPDX checksum. Lockfiles carrying one also load again instead of failing with `ERR_PNPM_BROKEN_LOCKFILE` [#13042](https://github.com/pnpm/pnpm/issues/13042).

  `pnpm sbom` now also publishes the checksum of a `type: binary` runtime archive, which pnpm does verify.

- A git dependency whose `git ls-remote` fails now reports the `ERR_PNPM_GIT_RESOLVE_FAILED` code, naming the dependency instead of printing a bare `git` invocation, with credentials in the repository URL redacted. A specifier that does not ask for SSH resolves over HTTPS, because the URL recorded in the lockfile has to work on every machine that installs it, so the error explains how to substitute the transport on a machine that can only reach the host over SSH (`git config --global url."git@<host>:".insteadOf "https://<host>/"`) [#13743](https://github.com/pnpm/pnpm/issues/13743).

  A missing `git` executable is reported as one, instead of surfacing the raw failure to start the process.

  Credentials embedded in a git specifier are redacted from the "Could not resolve \<ref\> to a commit of \<repo\>" errors too.

  Resolving a public repository makes one `git ls-remote` round-trip instead of two.

- `pnpm install` after moving a dependency between `dependencies`, `devDependencies`, and `optionalDependencies` now updates the lockfile in place instead of re-resolving the whole dependency graph [#13696](https://github.com/pnpm/pnpm/issues/13696).

- `syncInjectedDepsAfterScripts` no longer fails with `ERR_PNPM_UNSUPPORTED_INODE_TYPE` when a workspace package contains an inode that is neither a file nor a directory, such as the FIFO 1Password's environments create for `.env`. Such an inode cannot be hardlinked into the injected copy, so it is skipped and the rest of the package still syncs [#13550](https://github.com/pnpm/pnpm/issues/13550).

  `syncInjectedDepsAfterScripts` also no longer fails with `EEXIST` when a workspace package replaced a file with a directory of the same name since the injected copy was last synced.

- `syncInjectedDepsAfterScripts` no longer fails with `ENOTDIR` when a workspace package replaced a directory with a file of the same name and the injected copy still held that directory's contents.

- `syncInjectedDepsAfterScripts` now removes the bin link of a bin the script dropped. Previously only new bins were linked, so a build step that stopped declaring one left its shim behind, pointing at a command that was no longer there.

- `syncInjectedDepsAfterScripts` now identifies a file by its device as well as its inode number. An inode number is only unique within one filesystem, so on its own it could match an unrelated file on another device and leave that path stale in the injected copy.

- `pnpm store prune` no longer deletes the lockfile verification log. The log records which lockfile passed which supply-chain policies, so it stays valid across a prune of the store; keeping it lets the next install skip re-verifying an unchanged lockfile.

- Widening a dependency's range no longer leaves the project on an older version. The lockfile update now points the project at the highest version of that dependency already in the lockfile that satisfies the new range — matching what a full resolution records — instead of keeping the locked version whenever it happened to satisfy, which could leave a duplicate behind. A range change that only an already-locked version satisfies is now also handled without re-resolving [#13778](https://github.com/pnpm/pnpm/issues/13778).

- `resolutionMode` is no longer ignored when `minimumReleaseAge` is in effect. `lowest-direct` and `time-based` pick the lowest satisfying version of a direct dependency again; previously any active release-age cutoff — including the built-in default — silently forced the highest, so `resolutionMode` only worked when `minimumReleaseAge: 0` was set explicitly [#13752](https://github.com/pnpm/pnpm/issues/13752).

- Adding a package to a workspace no longer forces a full re-resolution when every dependency it declares is already locked for a sibling. The lockfile update writes the new project's importer entry from the versions the lockfile already holds; a dependency no locked version satisfies still reaches the resolver [#13696](https://github.com/pnpm/pnpm/issues/13696).

- `pnpm config delete <key>` no longer fails with `ENOENT` when the config file it would edit does not exist. Clearing a setting that was never set is a no-op [#13651](https://github.com/pnpm/pnpm/issues/13651).

- Changing a `pnpm.overrides` entry to a version range now updates the lockfile in place when a version the lockfile already holds satisfies the range, instead of re-resolving the whole dependency graph. Only exact versions were handled before [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Changing a parent-scoped `pnpm.overrides` entry (`"parent>child": "2.0.0"`) now updates the lockfile in place instead of re-resolving the whole dependency graph. Only the named parent's dependency moves; every other package keeps the version it had [#13795](https://github.com/pnpm/pnpm/issues/13795).

- Removing a dependency, or moving one to another already-locked version, no longer re-resolves the whole dependency graph just because some package resolves a peer with the same name. The lockfile update now compares the peer suffixes against the exact `name@version` the removal severed, so a suffix that names a different — still present — version of that dependency is left alone [#13781](https://github.com/pnpm/pnpm/issues/13781).

- Projects with a pnpmfile now use the fast lockfile update paths: an unchanged pnpmfile (proven by the recorded `pnpmfileChecksum`) no longer forces a full re-resolution for removals, dependency group moves, compatible range changes, and the other in-place lockfile rewrites [#13696](https://github.com/pnpm/pnpm/issues/13696).

- A lockfile entry whose resolution is unchanged no longer loses its recorded `deprecated` marker when a registry serves the package's metadata inconsistently — re-resolving to the same version keeps the deprecation instead of silently dropping the line [#13846](https://github.com/pnpm/pnpm/issues/13846).

- `pnpm prune` is now recursive by default inside a workspace, just like `pnpm install`. This fixes `pnpm prune --prod` in a workspace root emptying the `node_modules` directories of the other workspace projects, dropping the links to the workspace packages they depend on in production [#13718](https://github.com/pnpm/pnpm/issues/13718).

- A setting written in kebab-case in the global `config.yaml` is now reported instead of being silently ignored [#13650](https://github.com/pnpm/pnpm/issues/13650).

- `pnpm remove` no longer re-resolves the dependency graph. The removed dependency's entries are dropped from `pnpm-lock.yaml` and anything they made unreachable is pruned, without registry access. The install still falls back to a full resolution when a surviving package resolves a peer dependency through the removed one.

- Removing a package from a workspace no longer forces a full re-resolution. The lockfile update drops the departed project's importer entry and prunes whatever only it depended on. A project that is still linked from a surviving project continues to be reported as an error [#13696](https://github.com/pnpm/pnpm/issues/13696).

- An install sharing a global virtual store no longer removes an incomplete package directory that another importer is still writing, which could fail with `failed to remove existing directory ... prior to swap: Directory not empty`. Such a directory is now repaired in place, and a package file left damaged by an interrupted install is restored instead of being kept.

- `pnpm sbom` no longer emits components for optional platform-specific dependencies that cannot be installed on the current platform (for example, the native `@rolldown/binding-*` variants for other operating systems). Such packages are present in the lockfile but are never downloaded, so their license (and other metadata) could not be resolved and they appeared in the SBOM without one. `pnpm sbom --lockfile-only` still describes the whole lockfile graph, which is platform-independent by design.

- An `ssh://` git dependency pointing at a bracketed IPv6 host, such as `ssh://[::1]/repo.git`, is resolved now. Its colons were read as an SCP-style path separator, which turned the address into `[:/1]` and left the specifier unresolvable. Applies to both the TypeScript CLI and pacquet.

  In the TypeScript CLI, an `ssh://` git dependency written without user info — `ssh://git.example.com/team/repo.git`, `git+ssh://git.example.com:2222/team/repo.git` — no longer fails with `TypeError: Cannot read properties of undefined (reading 'includes')`. Only the `user@host` form worked before.

- `packageExtensions` is now validated when the configuration is read, so a malformed entry (for instance a dependency range set to `null`) fails with an actionable error instead of crashing later during peer dependency resolution [#13756](https://github.com/pnpm/pnpm/issues/13756).

- Projects using `resolutionMode: time-based` now benefit from the fast lockfile update paths. A removal, a dependency group move, or a compatible range change no longer forces a full re-resolution just because the lockfile carries a `time` field [#13696](https://github.com/pnpm/pnpm/issues/13696).

- An install that drops the last dependent of a patched package no longer updates the lockfile in place and succeeds silently. Removing a dependency, widening `ignoredOptionalDependencies`, or adding a removal override could each prune the package while the patch stayed configured; such an install now falls back to a full resolution, which reports the unused patch with `ERR_PNPM_UNUSED_PATCH`. Under `allowUnusedPatches`, where the lockfile update is kept, the same install now warns that the patch went unused instead of saying nothing [#13827](https://github.com/pnpm/pnpm/issues/13827).
