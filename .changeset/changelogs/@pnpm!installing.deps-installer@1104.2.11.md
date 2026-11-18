## 1104.2.11

### Patch Changes

- `pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`. `pnpm clean` and `pnpm ci` also clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.

- `pnpm install` now re-verifies lockfile resolutions when the default registry or a scoped registry changes [#18070](https://github.com/pnpm/pnpm/pull/18070).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.43
  - @pnpm/bins.remover@1100.0.33
  - @pnpm/building.after-install@1103.0.17
  - @pnpm/building.during-install@1102.2.14
  - @pnpm/building.policy@1100.1.11
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/config.package-is-installable@1100.2.7
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.graph-hasher@1100.3.13
  - @pnpm/deps.path@1101.0.10
  - @pnpm/exec.lifecycle@1100.1.30
  - @pnpm/fs.symlink-dependency@1100.0.27
  - @pnpm/hooks.read-package-hook@1100.3.13
  - @pnpm/hooks.types@1101.0.9
  - @pnpm/installing.context@1101.0.17
  - @pnpm/installing.deps-resolver@1102.2.14
  - @pnpm/installing.deps-restorer@1103.2.11
  - @pnpm/installing.linking.direct-dep-linker@1100.0.27
  - @pnpm/installing.linking.hoist@1100.0.44
  - @pnpm/installing.linking.modules-cleaner@1100.1.35
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/installing.package-requester@1102.2.11
  - @pnpm/lockfile.filtering@1100.2.18
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.preferred-versions@1100.0.44
  - @pnpm/lockfile.pruner@1100.0.31
  - @pnpm/lockfile.settings-checker@1100.2.19
  - @pnpm/lockfile.to-pnp@1101.0.16
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/lockfile.verification@1100.1.18
  - @pnpm/lockfile.walker@1100.0.31
  - @pnpm/network.auth-header@1101.1.19
  - @pnpm/patching.config@1100.1.13
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/pnpr.client@3.1.9
  - @pnpm/resolving.local-resolver@1101.2.10
  - @pnpm/resolving.npm-resolver@1104.2.11
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/store.controller-types@1101.3.7
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
