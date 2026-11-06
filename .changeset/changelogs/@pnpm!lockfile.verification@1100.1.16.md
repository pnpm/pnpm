## 1100.1.16

### Patch Changes

- `pnpm install --frozen-lockfile`, `pnpm approve-builds`, and `pnpm rebuild` now accept a lockfile that resolves a `*` dependency to a prerelease `latest` version. pnpm wrote such a lockfile and then reported it as outdated [#17594](https://github.com/pnpm/pnpm/issues/17594).

- `pnpm install --frozen-lockfile` now accepts a lockfile written with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE` once `node_modules` was removed.

- `pnpm install --frozen-lockfile` now accepts the lockfile it wrote with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a plain version range. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`. The frozen install now links the workspace package again.

- Updated dependencies:
  - @pnpm/installing.context@1101.0.15
  - @pnpm/installing.deps-resolver@1102.2.12
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/resolving.npm-resolver@1104.2.9
