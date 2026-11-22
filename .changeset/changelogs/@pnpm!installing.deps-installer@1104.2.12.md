## 1104.2.12

### Patch Changes

- Frozen installs now detect changes to catalog entries used only by peer dependencies of local directory packages.

- `pnpm install --dry-run` no longer runs the root project's `pnpm:devPreinstall` script.

- `pnpm install` now runs the workspace root's `pnpm:devPreinstall` script when `sharedWorkspaceLockfile` is `false` [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- `pnpm install` no longer fails without a TTY when it has to recreate an incompatible `node_modules` directory, such as in a Docker build or with piped stdin. It now recreates the directory and logs a message. A custom or symlinked modules directory still requires setting `confirmModulesPurge` to `false` [#6778](https://github.com/pnpm/pnpm/issues/6778), [#9166](https://github.com/pnpm/pnpm/issues/9166), [#8654](https://github.com/pnpm/pnpm/issues/8654), [#8085](https://github.com/pnpm/pnpm/issues/8085).

- Added the `ignoreLockfileSettingsChecks` install option to the programmatic API. When it is enabled, pnpm does not compare the `settings` in `pnpm-lock.yaml` with the current configuration, so a frozen install no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` and other installs no longer re-resolve because of a mismatch.

- A filtered `pnpm install` that changes the installed dependency groups, such as `pnpm --filter app install --prod`, no longer empties the `node_modules` directories of the workspace projects it skips.

- When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.

- `pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.

- `pnpm install` with `pnprServer` configured now falls back to local resolution when the project declares `file:`, `link:`, or local path dependencies. `pnpr` also rejects local filesystem dependency specifiers on `/-/pnpr/v0/resolve`.

- `pnpm install` now records the supply-chain verification of the lockfile it writes by the lockfile's content. Before, a lockfile that another process put in its place right after the write could reuse that verification on the next install.

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- Print a warning explaining why a modules directory has to be removed and recreated.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.44
  - @pnpm/bins.remover@1100.0.34
  - @pnpm/building.after-install@1103.0.18
  - @pnpm/building.during-install@1102.2.15
  - @pnpm/building.policy@1100.1.12
  - @pnpm/catalogs.config@1100.0.11
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/config.package-is-installable@1100.2.8
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/crypto.hash@1100.0.10
  - @pnpm/deps.graph-hasher@1100.3.14
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/exec.lifecycle@1100.1.31
  - @pnpm/fs.read-modules-dir@1100.0.5
  - @pnpm/fs.symlink-dependency@1100.0.28
  - @pnpm/hooks.read-package-hook@1100.3.14
  - @pnpm/hooks.types@1101.0.10
  - @pnpm/installing.context@1101.0.18
  - @pnpm/installing.deps-resolver@1102.3.0
  - @pnpm/installing.deps-restorer@1103.2.12
  - @pnpm/installing.linking.direct-dep-linker@1100.0.28
  - @pnpm/installing.linking.hoist@1100.0.45
  - @pnpm/installing.linking.modules-cleaner@1100.1.36
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/installing.package-requester@1102.2.12
  - @pnpm/lockfile.filtering@1100.2.19
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.preferred-versions@1100.0.45
  - @pnpm/lockfile.pruner@1100.0.32
  - @pnpm/lockfile.settings-checker@1100.2.20
  - @pnpm/lockfile.to-pnp@1101.0.17
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/lockfile.verification@1100.1.19
  - @pnpm/lockfile.walker@1100.0.32
  - @pnpm/network.auth-header@1101.1.20
  - @pnpm/patching.config@1100.1.14
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/pnpr.client@3.1.10
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/store.controller-types@1101.3.8
  - @pnpm/store.index@1101.0.5
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
