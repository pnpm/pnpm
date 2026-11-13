## 12.21.0

### Minor Changes

- `pnpm store add` run in a project now prepares downloaded tarballs for every lockfile entry that pins them. Besides registry packages, `pnpm install --offline --frozen-lockfile` can then install dependencies on a tarball URL and dependencies on a git host such as GitHub whose lockfile entry records an `integrity`.

### Patch Changes

- A `requirements.txt` beside a `pyproject.toml` that declares `[tool.uv.workspace]` without a `[project]` table no longer makes that directory a Python project. `pnpm install` stopped there with an error when the directory had a `.venv` that pnpm did not create [#18023](https://github.com/pnpm/pnpm/issues/18023).

- Fixed aliased dependencies sharing the wrong peer provider across workspace projects when a child uses another version of the aliased package as a peer.

- `pnpm install` with `pnprServer` now bounds response sizes from `/-/pnpr/v0/resolve` and `/-/pnpr/v0/verify-lockfile`. Oversized NDJSON frames, excessive frame counts, and large error bodies abort the exchange cleanly [zkochan/pnpm-tasks#76](https://github.com/zkochan/pnpm-tasks/issues/76).

- Fixed `pnpm install` changing optional peer providers on repeated installs when another version of the provider participates in a peer dependency cycle.

- Fixed cyclic peer dependencies sharing the wrong provider across workspace projects when the projects select different versions of that provider.

- `pnpm deploy` now binds optional peers declared only in `peerDependenciesMeta` for linked workspace packages. The deployed manifest and lockfile also retain these peers when filtering dependency groups.

- Fixed `pnpm deploy` failing with `lockfile.format: v13` when injected workspace packages have hashed peer bindings.

- Fixed repeated installs changing peer providers after a dependency update.

- Fixed creating `lockfile.format: v13` lockfiles when a package has many dependencies that it also declares as peers.

- Frozen installs with v13 lockfiles now accept valid peer dependencies of injected workspace packages that use catalogs.

- `pnpm fetch` with `nodeLinker` set to `hoisted` no longer populates global virtual store links. `pnpm fetch` also preserves an existing hoisted `node_modules` directory. `pnpm fetch` no longer modifies linked agent skills. `pnpm install` with `frozenStore` enabled no longer fails with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` when `nodeLinker` is `hoisted` [#18037](https://github.com/pnpm/pnpm/issues/18037).

- Fixed `pnpm install` creating duplicate copies of a package with identical resolved peers when `nodeLinker` is `hoisted` and `autoInstallPeers` is `false`. This happened when an ancestor depended on a different version of one of those peers [#18003](https://github.com/pnpm/pnpm/issues/18003).

- With `nodeLinker: hoisted` and `autoInstallPeers: false`, a package no longer loads a copy of its dependency that was resolved with different peer versions. This could happen when one package version had several copies with different peers in the dependency graph [#18004](https://github.com/pnpm/pnpm/pull/18004).

- Fixed injected workspace packages that depend on themselves linking to a nonexistent directory.

- An explicit `lockfileDir` that points to the default lockfile directory keeps the default install behavior [pnpm/tasks#275](https://github.com/pnpm/tasks/issues/275).

- pnpm now reads a lockfile with a mapping key longer than 1,024 characters written as `'key':`, without the `? ` marker. Before, `pnpm install` and other commands failed on such a lockfile with `ERR_PNPM_BROKEN_LOCKFILE`.

- `pnpm install` now uses package manifests when retaining peer versions from lockfiles with hashed peer suffixes.

- `pnpm pipeline` now rebuilds local packages and reruns build scripts after it restores a shared Cargo build cache with Cargo 1.100 or newer. Before, a worktree could reuse build-script output from the worktree that created the cache.

- If the pnpr server refuses a setting the install uses, or cannot return the lockfile format pnpm reads, `pnpm install` and `pnpm update` print a warning and resolve locally. Cargo and Python resolution also fall back to resolving locally. pnpm now rejects a pnpr resolution of npm dependencies that does not name its lockfile format.

- Fixed rewriting `lockfile.format: v13` lockfiles when a package has many dependencies that it also declares as peers.

- `pnpm install` now checks peer dependencies against package manifests when reusing a lockfile with hashed peer suffixes.

- pnpm now displays pnpr error codes and reasons when remote requests fail.

- Repeated installs now keep optional peer bindings stable after a workspace adds or removes peer providers [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- With `autoInstallPeers: false`, removing a dependency no longer restores it from an old peer resolution.

- Fixed repeated installs changing peer dependency bindings in workspaces with injected dependencies and `autoInstallPeers` disabled.

- Fixed repeated installs changing required peer dependency versions after adding a dependency.

- Removed direct dependencies no longer supply their old peer context when they are hoisted again.

- Refactored peer dependency processing during installs.

- `pnpm setup` and the install script now support PowerShell as the login shell on Linux and macOS. They add `PNPM_HOME` to `PATH` in `profile.ps1` under `$XDG_CONFIG_HOME/powershell` or `~/.config/powershell` [#18026](https://github.com/pnpm/pnpm/issues/18026).

- Fixed repeated installs changing peer versions in dependency cycles.

- Unchanged installs now preserve peer contexts when peer-dependent packages are deduplicated. Removing a workspace dependency preserves nested peer providers still required by the remaining packages.

  Unchanged installs retain valid locked peer providers even when a direct dependency provides another compatible version. Explicit `pnpm add` and `pnpm update` requests take precedence over those retained bindings.

- Repeated installs now keep automatically installed peer versions stable when workspace projects use different ranges of the same dependency. Adding another ranged provider also preserves a valid peer pin.

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Scripts now stop when pnpm's process group is killed while they are starting.
