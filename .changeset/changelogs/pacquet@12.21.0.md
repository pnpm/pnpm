## 12.21.0

pnpm 12.21.0 lets `pnpm store add` prepare tarball and git dependencies for offline installs. It also makes repeated installs keep their peer dependency choices and fixes several `lockfile.format: v13`, `nodeLinker: hoisted`, and `pnpm deploy` failures. If a pnpr server refuses a setting, pnpm now resolves locally.

### Minor Changes

- `pnpm store add` run in a project now prepares downloaded tarballs for every lockfile entry that pins them. Besides registry packages, `pnpm install --offline --frozen-lockfile` can then install dependencies on a tarball URL and dependencies on a git host such as GitHub whose lockfile entry records an `integrity`.

### Patch Changes

#### Installs that failed

- pnpm now reads a lockfile with a mapping key longer than 1,024 characters written as `'key':`, without the `? ` marker. Before, `pnpm install` and other commands failed on such a lockfile with `ERR_PNPM_BROKEN_LOCKFILE`.

- With `lockfile.format: v13`, `pnpm install` no longer fails to create or rewrite the lockfile when a package declares many of its dependencies as peers.

- Fixed `pnpm deploy` failing with `lockfile.format: v13` when injected workspace packages have hashed peer bindings.

- Frozen installs with v13 lockfiles now accept valid peer dependencies of injected workspace packages that use catalogs.

- Fixed injected workspace packages that depend on themselves linking to a nonexistent directory.

- `pnpm install` with `frozenStore` enabled no longer fails with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` when `nodeLinker` is `hoisted`. `pnpm fetch` with `nodeLinker: hoisted` no longer populates global virtual store links or modifies linked agent skills, and it preserves an existing hoisted `node_modules` directory [#18037](https://github.com/pnpm/pnpm/issues/18037).

- A `requirements.txt` beside a `pyproject.toml` that declares `[tool.uv.workspace]` without a `[project]` table no longer makes that directory a Python project. `pnpm install` stopped there with an error when the directory had a `.venv` that pnpm did not create [#18023](https://github.com/pnpm/pnpm/issues/18023).

#### Peer dependencies

- Repeated installs no longer change the peer dependency providers that the previous install chose. This covers adding, updating, or removing a dependency, projects that use different ranges of one dependency, dependency cycles, and injected dependencies with `autoInstallPeers: false` [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

  An unchanged install keeps a valid locked peer provider even when a direct dependency provides another compatible version. Explicit `pnpm add` and `pnpm update` requests take precedence over it.

- Workspace projects that select different versions of a peer provider no longer share one copy of the package that depends on it. This affected peers reached through an aliased dependency or a peer dependency cycle.

- A removed dependency no longer comes back as a peer provider. Before, a direct dependency removed with `autoInstallPeers: false` could be restored from an old peer resolution, and a removed one could still supply its old peer context.

- With `nodeLinker: hoisted` and `autoInstallPeers: false`, `pnpm install` no longer creates duplicate copies of a package whose resolved peers are identical. A package also no longer loads a copy of its dependency that was resolved with different peer versions [#18003](https://github.com/pnpm/pnpm/issues/18003).

- `pnpm deploy` now binds optional peers declared only in `peerDependenciesMeta` for linked workspace packages. The deployed manifest and lockfile also retain these peers when filtering dependency groups.

#### Running scripts

- Scripts now stop when pnpm's process group is killed while they are starting.

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- `pnpm pipeline` now rebuilds local packages and reruns build scripts after it restores a shared Cargo build cache with Cargo 1.100 or newer. Before, a worktree could reuse build-script output from the worktree that created the cache.

#### pnpr

- If the pnpr server refuses a setting the install uses, or cannot return the lockfile format pnpm reads, `pnpm install` and `pnpm update` print a warning and resolve locally. Cargo and Python resolution also fall back to resolving locally. pnpm now rejects a pnpr resolution of npm dependencies that does not name its lockfile format.

- `pnpm install` with `pnprServer` now limits the size of the server's resolve and lockfile verification responses. An oversized response ends the request with an error.

- pnpm now displays pnpr error codes and reasons when remote requests fail.

#### Other commands and settings

- `pnpm setup` and the install script now support PowerShell as the login shell on Linux and macOS. They add `PNPM_HOME` to `PATH` in `profile.ps1` under `$XDG_CONFIG_HOME/powershell` or `~/.config/powershell` [#18026](https://github.com/pnpm/pnpm/issues/18026).

- A `lockfileDir` setting that points at the default lockfile directory now installs the same way as leaving it unset [pnpm/tasks#275](https://github.com/pnpm/tasks/issues/275).
