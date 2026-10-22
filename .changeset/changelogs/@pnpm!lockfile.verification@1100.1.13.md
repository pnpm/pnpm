## 1100.1.13

### Patch Changes

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when an injected workspace package depends on a local directory under another name, such as `"renamed": "file:../b"` [#17130](https://github.com/pnpm/pnpm/issues/17130).

- Fixed `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` failing inside a directory created by `pnpm deploy`. `pnpm deploy` now writes self-contained workspace and lockfile settings. `pnpm deploy` also prunes unused `patchedDependencies` and preserves workspace-level dependency resolutions on deployed directory dependencies.

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of an injected workspace package no longer match the lockfile. Before, it installed the stale dependencies when `sharedWorkspaceLockfile` was `false` [#17008](https://github.com/pnpm/pnpm/issues/17008).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a local directory dependency, such as an injected workspace package, changes a dependency to a different tarball URL or git commit [#17129](https://github.com/pnpm/pnpm/issues/17129).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of a transitive injected or local directory dependency no longer match the lockfile, even when that dependency is not a workspace importer. Before, only the immediate directory dependency's children were checked, so a stale transitive snapshot was installed silently [#17128](https://github.com/pnpm/pnpm/issues/17128).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.7
  - @pnpm/installing.context@1101.0.12
  - @pnpm/installing.deps-resolver@1102.2.9
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/resolving.git-resolver@1100.1.28
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
