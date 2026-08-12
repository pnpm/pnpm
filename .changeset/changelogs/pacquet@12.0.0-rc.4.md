## 12.0.0-rc.4

### Minor Changes

- Added a new setting `minimumReleaseAgeExcludePrune`. When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

  Renamed `cleanupUnusedCatalogs` to `catalogPrune`, so that catalog pruning and release-age exclude pruning use one vocabulary. `cleanupUnusedCatalogs` continues to work; when both are set, `catalogPrune` wins.

- Added support for the `syncInjectedDepsAfterScripts` setting. It names the scripts after which every injected copy of the package that ran them is brought back in step with its source, so a build script no longer leaves the copies in the virtual store holding stale files.

### Patch Changes

- `pnpm add` no longer re-resolves the dependency graph when `pnpm-lock.yaml` already holds a version satisfying the request — promoting a transitive dependency to a direct one, or adding to a second workspace package what a first one already depends on, now only saves the dependency in `package.json` and records its importer entry. A satisfying locked version is necessary but not sufficient: the install still falls back to a full resolution for a dist tag, an alias, a `workspace:`/`catalog:`/git/tarball specifier, `--save-peer`, an overridden package, a `catalogMode` other than `manual`, and — under `resolutionMode: time-based` or `lowest-direct`, which resolve a direct dependency to the low end of its range — a range several locked versions satisfy.

- Global installs now switch over atomically. The command shims in the global bin directory point at a stable per-package link rather than at the directory a particular install produced, so `pnpm add -g` and `pnpm update -g` activate a new version by moving that one link instead of rewriting every shim. A command can no longer be missing from `PATH` while an install is in progress, and a failed install leaves the previous version in place.

- `pnpm audit --fix` and `pnpm audit --fix update` no longer add `minimumReleaseAgeExclude` entries for patched versions that were published before the `minimumReleaseAge` cutoff. The publish time of each minimum patched version is now checked against the registry metadata, and only versions young enough to be blocked by the age gate get an exclusion entry [#11563](https://github.com/pnpm/pnpm/issues/11563).

- Bounded the number of requests in flight to the `.pnpmfile.cjs` worker process. An install that runs the `readPackage` hook for thousands of packages at once no longer risks failing with `ERR_PNPM_PNPMFILE_FAIL` on a hook timeout spent waiting in the queue rather than running the hook, and holds fewer copies of the manifests it is hooking while it waits.

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` under `catalogMode: strict` no longer fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` when the catalog entry is a range that the wanted version satisfies. The dependency keeps using the catalog; only a version that really falls outside the catalog's range is rejected [#13715](https://github.com/pnpm/pnpm/issues/13715).

- Fixed `pnpm install` in CI to use frozen lockfile mode by default when an existing `pnpm-lock.yaml` is non-empty. An outdated lockfile now fails without being rewritten, while projects without a lockfile or with an empty lockfile can still generate one.

- A changed `catalogs` or `pnpm.overrides` block no longer has to be the only change for `pnpm install` to update the lockfile in place. Editing an override while also removing a dependency, or changing a catalog entry in the same commit as a range bump, is now absorbed in one pass instead of re-resolving the whole dependency graph [#13799](https://github.com/pnpm/pnpm/issues/13799).

  Fixed the lockfile an in-place override update wrote when the overridden package was also a catalog entry: the entry kept the version it had before the override moved the package. The same could happen in reverse, when a catalog entry moved a package an override pins. Both cases now re-resolve instead.

- `pnpm install` now updates the lockfile in place even when several kinds of changes happened since the last install — for example a removed dependency together with a widened `ignoredOptionalDependencies` list, or a dependency edit alongside a patch or settings change. Previously any combination of changes forced a full re-resolution [#13763](https://github.com/pnpm/pnpm/issues/13763).

- Resolving peer dependencies in a workspace whose dependency graph contains many peer-dependency cycles now needs less than half the memory and finishes about twice as fast. Verdicts computed inside dependency cycles are now cached and reused for the occurrences they are provably valid for, instead of being recomputed for every occurrence.

- `pnpm install` and `pnpm dedupe` no longer eat all the available memory while resolving a graph in which many packages declare the same missing peer dependency, such as the `react` peer the `@radix-ui` packages share [#13786](https://github.com/pnpm/pnpm/issues/13786).

- With `dedupeDirectDeps`, a project's symlink that becomes redundant — because the workspace root started providing the same dependency at the same resolution — is removed on the next install instead of surviving forever [#13775](https://github.com/pnpm/pnpm/issues/13775). The layout no longer depends on install history: an incremental install now ends up with the same `node_modules` a clean install of the same manifests produces.

- `pnpm deploy` injects workspace dependencies again, so the deploy directory is self-contained instead of symlinking back into the source workspace [#13754](https://github.com/pnpm/pnpm/issues/13754). Enabling `injectWorkspacePackages` with `dedupeInjectedDeps` disabled now also rewrites already-linked workspace dependencies to injected copies.

- `pnpm deploy --no-optional` no longer writes a lockfile whose snapshots reference optional dependencies that the deploy excluded.

- `pnpm --filter . deploy` deploys the project in the current directory instead of the projects nested under it, so deploying the workspace root now copies the root project and installs its workspace dependencies [#13758](https://github.com/pnpm/pnpm/issues/13758). `pnpm deploy --legacy` no longer rewrites the source workspace's `pnpm-lock.yaml`.

- Fixed `pnpm install` writing a different `pnpm-lock.yaml` for an unchanged project depending on the order its dependencies happened to resolve in, which showed up as spurious lockfile diffs between installs.

- Removing the last dependency that references a catalog entry via the fast lockfile update no longer leaves the stale catalog entry in `pnpm-lock.yaml`.

- `--frozen-lockfile` no longer rejects a lockfile pnpm just generated when `packageExtensions` adds a peer dependency to a workspace project. The peer is auto-installed and recorded in the importer entry, but the freshness check compared against the `package.json` on disk, which has no such peer, and reported the entry as a removed dependency [#13836](https://github.com/pnpm/pnpm/issues/13836).

- A git dependency whose clone (or shallow fetch) fails now reports which package it belongs to, under the `ERR_PNPM_GIT_FETCH_FAILED` code, with credentials in the repository URL redacted. When the lockfile records an SSH remote, the error also explains that fetching it needs an SSH key for that host, and that a lockfile entry written before pnpm v11.21 can be re-recorded over HTTPS with `pnpm update <package>` [#13743](https://github.com/pnpm/pnpm/issues/13743).

- An `integrity` recorded on a git dependency's resolution (`resolution: {type: git, repo, commit, integrity: sha512-…}`) is no longer treated as a checksum. pnpm never verifies a git checkout against such a hash — the commit pins the content — so it is now dropped when the lockfile is rewritten, and `pnpm sbom` no longer republishes it as a CycloneDX/SPDX checksum. Lockfiles carrying one also load again instead of failing with `ERR_PNPM_BROKEN_LOCKFILE` [#13042](https://github.com/pnpm/pnpm/issues/13042).

  `pnpm sbom` now also publishes the checksum of a `type: binary` runtime archive, which pnpm does verify.

- A git dependency whose `git ls-remote` fails now reports the `ERR_PNPM_GIT_RESOLVE_FAILED` code, naming the dependency instead of printing a bare `git` invocation, with credentials in the repository URL redacted. A specifier that does not ask for SSH resolves over HTTPS, because the URL recorded in the lockfile has to work on every machine that installs it, so the error explains how to substitute the transport on a machine that can only reach the host over SSH (`git config --global url."git@<host>:".insteadOf "https://<host>/"`) [#13743](https://github.com/pnpm/pnpm/issues/13743).

  A missing `git` executable is reported as one, instead of surfacing the raw failure to start the process.

  Credentials embedded in a git specifier are redacted from the "Could not resolve \<ref\> to a commit of \<repo\>" errors too.

  Resolving a public repository makes one `git ls-remote` round-trip instead of two.

- `pnpm install` after moving a dependency between `dependencies`, `devDependencies`, and `optionalDependencies` now updates the lockfile in place instead of re-resolving the whole dependency graph [#13696](https://github.com/pnpm/pnpm/issues/13696).

- `--ignore-pnpmfile` is accepted again, on every command pnpm takes it on: `install`, `add`, `update`, `dedupe`, `fetch`, `unlink`, `deploy`, `ci`, and `install-test` [#13808](https://github.com/pnpm/pnpm/issues/13808). The flag skips every pnpmfile hook the command would otherwise run: neither the workspace `.pnpmfile.cjs` nor the pnpmfiles of config dependencies are loaded, so no `readPackage`, `updateConfig`, `afterAllResolved`, custom resolver, or custom fetcher runs.

- `syncInjectedDepsAfterScripts` now removes the bin link of a bin the script dropped. Previously only new bins were linked, so a build step that stopped declaring one left its shim behind, pointing at a command that was no longer there.

- Fixed dependency resolution letting the order in which concurrent resolutions finished decide the outcome. When one package was reached from several places, whichever occurrence got there first decided the versions its dependencies were recorded at, so repeated installs of the same project could produce different `pnpm-lock.yaml` files.

- Widening a dependency's range no longer leaves the project on an older version. The lockfile update now points the project at the highest version of that dependency already in the lockfile that satisfies the new range — matching what a full resolution records — instead of keeping the locked version whenever it happened to satisfy, which could leave a duplicate behind. A range change that only an already-locked version satisfies is now also handled without re-resolving [#13778](https://github.com/pnpm/pnpm/issues/13778).

- The lockfile's `time:` section is no longer dropped when `pnpm-lock.yaml` is rewritten. `resolutionMode: time-based` records each direct dependency's publish date there and now reads it back as the fallback for a package whose registry metadata carries no publish date, so a later resolution derives the same cutoff instead of picking different subdependency versions [#13776](https://github.com/pnpm/pnpm/issues/13776).

- `resolutionMode` is no longer ignored when `minimumReleaseAge` is in effect. `lowest-direct` and `time-based` pick the lowest satisfying version of a direct dependency again; previously any active release-age cutoff — including the built-in default — silently forced the highest, so `resolutionMode` only worked when `minimumReleaseAge: 0` was set explicitly [#13752](https://github.com/pnpm/pnpm/issues/13752).

- Adding a package to a workspace no longer forces a full re-resolution when every dependency it declares is already locked for a sibling. The lockfile update writes the new project's importer entry from the versions the lockfile already holds; a dependency no locked version satisfies still reaches the resolver [#13696](https://github.com/pnpm/pnpm/issues/13696).

- Changing a `pnpm.overrides` entry to a version range now updates the lockfile in place when a version the lockfile already holds satisfies the range, instead of re-resolving the whole dependency graph. Only exact versions were handled before [#13696](https://github.com/pnpm/pnpm/issues/13696).

- `pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` now move a catalog entry's resolution to the requested version. Previously, when the catalog entry was a range that covered the requested version but resolved to a different one, the request was dropped silently: nothing was installed, nothing was written, and no error was raised.

- Changing a parent-scoped `pnpm.overrides` entry (`"parent>child": "2.0.0"`) now updates the lockfile in place instead of re-resolving the whole dependency graph. Only the named parent's dependency moves; every other package keeps the version it had [#13795](https://github.com/pnpm/pnpm/issues/13795).

- Reduced peak memory usage while resolving peer dependencies. Workspaces with large, deeply peer-dependent dependency graphs could need gigabytes to install; the same install now needs meaningfully less.

- Removing a dependency, or moving one to another already-locked version, no longer re-resolves the whole dependency graph just because some package resolves a peer with the same name. The lockfile update now compares the peer suffixes against the exact `name@version` the removal severed, so a suffix that names a different — still present — version of that dependency is left alone [#13781](https://github.com/pnpm/pnpm/issues/13781).

- `pnpm install` no longer re-resolves dependencies inside a subtree the lockfile pinned when another dependency reaches the same package. Those packages kept their locked versions in `node_modules` while `pnpm-lock.yaml` recorded newer ones, so an install could quietly move a transitive dependency — including across a major version — without anything asking it to.

- A `.pnpmfile.cjs` `readPackage` hook that rewrites one of a project's *own* dependency specifiers is now honored: rewriting `"is-positive": "^1.0.0"` to `1.0.0` installs 1.0.0 and records `specifier: 1.0.0` for the importer. Previously the hook was applied only to the manifests of resolved dependencies, so a project's own specifier resolved against the raw range from `package.json` [#13769](https://github.com/pnpm/pnpm/issues/13769).

- `pnpm prune` now prints the `Scope: all N workspace projects` line when run inside a workspace, as it prunes every project of the workspace.

- Removing a package from a workspace now drops its importer entry from `pnpm-lock.yaml`, along with the dependencies only it needed. Previously the entry survived every later install, which kept those dependencies reachable and made the lockfile diverge from the one the TypeScript CLI writes [#13783](https://github.com/pnpm/pnpm/issues/13783).

- `pnpm remove` no longer re-resolves the dependency graph. The removed dependency's entries are dropped from `pnpm-lock.yaml` and anything they made unreachable is pruned, without registry access. The install still falls back to a full resolution when a surviving package resolves a peer dependency through the removed one.

- An install sharing a global virtual store no longer removes an incomplete package directory that another importer is still writing, which could fail with `failed to remove existing directory ... prior to swap: Directory not empty`. Such a directory is now repaired in place, and a package file left damaged by an interrupted install is restored instead of being kept.

- `pnpm sbom` no longer emits components for optional platform-specific dependencies that cannot be installed on the current platform (for example, the native `@rolldown/binding-*` variants for other operating systems). Such packages are present in the lockfile but are never downloaded, so their license (and other metadata) could not be resolved and they appeared in the SBOM without one. `pnpm sbom --lockfile-only` still describes the whole lockfile graph, which is platform-independent by design.

- Kept unselected workspace link targets shallow during filtered isolated installs.

- Reduced peak memory usage while resolving peer dependencies further: each occurrence in the dependency tree now shares its package id with the edge it came from instead of owning a copy of it.

- An `ssh://` git dependency pointing at a bracketed IPv6 host, such as `ssh://[::1]/repo.git`, is resolved now. Its colons were read as an SCP-style path separator, which turned the address into `[:/1]` and left the specifier unresolvable. Applies to both the TypeScript CLI and pacquet.

  In the TypeScript CLI, an `ssh://` git dependency written without user info — `ssh://git.example.com/team/repo.git`, `git+ssh://git.example.com:2222/team/repo.git` — no longer fails with `TypeError: Cannot read properties of undefined (reading 'includes')`. Only the `user@host` form worked before.

- Commands in a project that pins a pnpm version no longer read the whole `pnpm-lock.yaml` to get at the leading env document. Reading stops at the end of that document, so the cost no longer grows with the rest of the lockfile: reading the env document out of an 8 MB lockfile takes ~15µs instead of ~390µs.

- An install that drops the last dependent of a patched package no longer updates the lockfile in place and succeeds silently. Removing a dependency, widening `ignoredOptionalDependencies`, or adding a removal override could each prune the package while the patch stayed configured; such an install now falls back to a full resolution, which reports the unused patch with `ERR_PNPM_UNUSED_PATCH`. Under `allowUnusedPatches`, where the lockfile update is kept, the same install now warns that the patch went unused instead of saying nothing [#13827](https://github.com/pnpm/pnpm/issues/13827).
