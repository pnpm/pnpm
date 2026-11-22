## 12.0.0-alpha.16

### Minor Changes

- Completed pnpm runtime installation parity for Node.js, Deno, and Bun, including runtime failure policy, target architecture selection, and dependency runtime engines. Runtime failure overrides now preserve explicit runtime dependencies without matching engine entries.

- Deprecated packages are reported during installation: a directly depended-on deprecated package gets an immediate warning, and deprecated subdependencies are summarized in a single `<N> deprecated subdependencies found` line. Versions matched by `pnpm.allowedDeprecatedVersions` are not warned about [#11633](https://github.com/pnpm/pnpm/issues/11633).

- Implemented native `install-test` command.

- Implemented native `recursive`, `multi`, and `m` commands in the Rust CLI.

- Added the `virtualStoreOnly` setting, which populates the virtual store without any post-import linking — no importer symlinks, no `.bin` entries, no hoisting, and no project lifecycle scripts. Combining it with `enableModulesDir: false` fails with `ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR` unless `enableGlobalVirtualStore` is on, since the standard virtual store lives inside `node_modules`. A subsequent ordinary install completes the linking instead of treating the partially-populated directory as up-to-date. `enableModulesDir` is now read from `pnpm-workspace.yaml` as well.

- Repeat installs now reconcile the existing `node_modules` the way the TypeScript CLI does: direct dependencies removed from the lockfile lose their links and bin shims, hoisted aliases of removed packages are unlinked so the next hoist pass can claim their slots, a hand-deleted package is detected and re-installed even when the lockfile is otherwise up to date, and `pnpm add` / `pnpm remove` fail with `ERR_PNPM_HOIST_PATTERN_DIFF`-family errors instead of silently recreating a modules directory whose layout settings drifted. Dev-only installs also no longer delete `node_modules/.pnpm/lock.yaml`.

- `pnpm install --ignore-scripts` now records the builds it skipped in `node_modules/.modules.yaml`'s `pendingBuilds`, and `pnpm rebuild --pending` runs them and clears the record instead of finding nothing to do. Both the dependencies whose build scripts were suppressed and the workspace projects whose own install scripts were suppressed are recorded and re-run, and an install that removes a package drops it from the list.

- `pnpm install` now fails with `ERR_PNPM_UNUSED_PATCH` when an entry in `patchedDependencies` doesn't match any installed package. Set `allowUnusedPatches: true` in `pnpm-workspace.yaml` to get a warning instead, matching pnpm 11 [#11633](https://github.com/pnpm/pnpm/issues/11633).

### Patch Changes

- `pnpm add` no longer drops the other dependency groups from the install: adding a package with `optionalDependencies` no longer leaves dangling optional-dependency symlinks in the virtual store (`pnpm add -g @openai/codex` produced a `codex` bin that failed with "Missing optional dependency `@openai/codex-darwin-arm64`"), and a production `pnpm add` no longer removes the project's `devDependencies` from `pnpm-lock.yaml` and `node_modules`.

- `pnpm add` with `--save-dev`, `--save-optional`, or `--save-prod` now moves an already-declared dependency to the target group instead of leaving a duplicate entry in its old group, matching pnpm.

- `pnpm add <pkg>` without a `--save-*` flag now updates an already-declared dependency in the group it occupies (`devDependencies` / `optionalDependencies`), matching pnpm, instead of always saving it into `dependencies`.

- `pnpm install` now detects a `supportedArchitectures` change and re-evaluates previously skipped platform-specific optional dependencies, instead of reporting the project as up to date and leaving the packages for the old architecture set in place.

- Avoided optimistic repeat-install shortcuts when a lockfile contains merge conflict markers.

- `pnpm setup` now removes leftover v10-layout shims at the top of `PNPM_HOME`, so `pnpm self-update` no longer warns about a v10 installation layout after PATH has been migrated to the v11 `PNPM_HOME/bin` layout. Applies to both the TypeScript CLI and pacquet.

  In the TypeScript CLI, `self-update` also no longer treats a dangling legacy shim (one whose install target was garbage-collected) as a real v10 layout, so the warning can no longer fire on dead shim files.

  Closes pnpm/pnpm#12496.

- A git-hosted dependency with no host archive (an ssh, self-hosted, or `git+file:` repo) whose package name matches the dependency's alias now records the bare `git+<repo>#<commit>` reference in the lockfile's importer entry, matching pnpm's `pnpm-lock.yaml` output instead of prefixing it with `<name>@`.

- A private git-hosted dependency resolved over HTTPS with an embedded auth token (`git+https://<token>@github.com/owner/repo.git`) is now recorded as a `type: git` resolution against the authenticated remote, instead of being rewritten to the host's public archive URL (a `codeload.github.com` tarball) that carries none of those credentials and so could not be fetched.

- When a dependency's build script fails under `enableGlobalVirtualStore`, the global virtual store directory it was being built in is now removed for scoped packages too. Previously the cleanup resolved one directory level short of the hash directory for a scoped name, leaving a half-built directory behind that later installs would reuse.

- A hoisted-linker install no longer fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` when an optional dependency's snapshot is absent because it was skipped on a previous install.

- Fixed patched dependencies being applied to only one copy of a package under `nodeLinker: hoisted`. When a version conflict kept a patched package out of the root `node_modules`, the hoisted layout nested a copy of it under each consumer that needed it, but only the first copy was patched — every other copy silently ran the unpatched code the patch existed to replace. The same gap applied to a reinstall served from the side-effects cache. Every copy is now patched, matching `nodeLinker: isolated` and pnpm's behavior.

- `pnpm install` now announces `Lockfile is up to date, resolution step is skipped` whenever the headless installer runs — including installs that materialize a cold `node_modules` from an up-to-date lockfile and `--filter` subset installs — matching the TypeScript CLI. `pnpm fetch` prints `Importing packages to virtual store` on that path instead.

- Conditional metadata requests send `If-Modified-Since` as an HTTP-date instead of the mirror's raw ISO-8601 `modified` value, so registries can answer `304 Not Modified` instead of re-serving the full packument [#13104](https://github.com/pnpm/pnpm/issues/13104).

- Fixed two global-virtual-store correctness gaps. A failed build now discards the hash directory it was building in, so the next install re-fetches instead of reusing a half-built directory shared by every project with the same dependency graph. The removal only ever touches a slot strictly inside the store, so a crafted package name cannot make it escape. And a side-effects-cache hit no longer assumes the store slot still holds the cached build: when the slot has been re-imported pristine, the build output is materialized rather than skipped, which previously left the package without its build artifacts.

  `.modules.yaml` now records the `allowBuilds` set the install ran under, matching pnpm.

- Aligned large-download progress byte formatting with pnpm.

- Changing `--os` / `--cpu` / `--libc` or `supportedArchitectures` between installs now re-evaluates previously skipped optional dependencies, so the platform packages for the newly selected architecture are installed instead of staying skipped.

- Removing a package from `allowBuilds` now fails the next `pnpm install` under `strictDepBuilds` instead of reporting the project as already up to date. A build whose output is already cached in the store no longer counts as an approval [#11035](https://github.com/pnpm/pnpm/issues/11035).

- `.modules.yaml` now records the dependencies of a skipped optional package in `skipped` as well, matching pnpm: when a platform-incompatible optional package is skipped, its own dependency subtree is not materialized either.

- Fixed proxy settings from the global `config.yaml` and command-line options in pnpm.
