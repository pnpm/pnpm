## 1100.1.34

### Patch Changes

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

  Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.

- `pnpm install --frozen-lockfile` now accepts a lockfile written with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE` once `node_modules` was removed.

- `pnpm install --frozen-lockfile` now accepts the lockfile it wrote with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a plain version range. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`. The frozen install now links the workspace package again.

- `verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a single-project repository changed. A script run after such a change no longer starts an install first [#17560](https://github.com/pnpm/pnpm/issues/17560).

- With `verifyDepsBeforeRun`, `pnpm run` and `pnpm exec` now report dependencies as out of date after a dependency is added to the `package.json` of a local directory dependency, such as `"lib": "file:../lib"`. Before, the command ran without the new dependency installed.

- `verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a workspace project changed and no dependency can resolve to that project. A script run after such a change no longer starts an install first.

- Updated dependencies:
  - @pnpm/hooks.read-package-hook@1100.3.11
  - @pnpm/installing.context@1101.0.15
  - @pnpm/lockfile.fs@1100.2.17
  - @pnpm/lockfile.settings-checker@1100.2.17
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/lockfile.verification@1100.1.16
  - @pnpm/workspace.state@1100.0.54
