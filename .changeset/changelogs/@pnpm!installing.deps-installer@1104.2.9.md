## 1104.2.9

### Patch Changes

- Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

  Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.

- `pnpm install --frozen-lockfile` now accepts a lockfile written with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE` once `node_modules` was removed.

- `pnpm install --frozen-lockfile` now accepts the lockfile it wrote with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a plain version range. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`. The frozen install now links the workspace package again.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.41
  - @pnpm/bins.remover@1100.0.31
  - @pnpm/building.after-install@1103.0.15
  - @pnpm/building.during-install@1102.2.12
  - @pnpm/deps.graph-hasher@1100.3.11
  - @pnpm/deps.graph-sequencer@1101.0.1
  - @pnpm/exec.lifecycle@1100.1.28
  - @pnpm/hooks.read-package-hook@1100.3.11
  - @pnpm/installing.context@1101.0.15
  - @pnpm/installing.deps-resolver@1102.2.12
  - @pnpm/installing.deps-restorer@1103.2.9
  - @pnpm/installing.linking.hoist@1100.0.42
  - @pnpm/installing.linking.modules-cleaner@1100.1.33
  - @pnpm/installing.package-requester@1102.2.9
  - @pnpm/lockfile.filtering@1100.2.16
  - @pnpm/lockfile.fs@1100.2.17
  - @pnpm/lockfile.preferred-versions@1100.0.42
  - @pnpm/lockfile.settings-checker@1100.2.17
  - @pnpm/lockfile.to-pnp@1101.0.14
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/lockfile.verification@1100.1.16
  - @pnpm/pnpr.client@3.1.7
  - @pnpm/resolving.npm-resolver@1104.2.9
