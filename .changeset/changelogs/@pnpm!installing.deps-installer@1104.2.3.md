## 1104.2.3

### Patch Changes

- `pnpm install --frozen-lockfile` again succeeds when a workspace project recorded in `pnpm-lock.yaml` has no directory, such as a project left out of a Docker build context. It still fails if the project's directory exists without a `package.json` [#16453](https://github.com/pnpm/pnpm/issues/16453).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.36
  - @pnpm/bins.remover@1100.0.27
  - @pnpm/building.after-install@1103.0.9
  - @pnpm/building.during-install@1102.2.6
  - @pnpm/building.policy@1100.1.5
  - @pnpm/config.matcher@1100.0.3
  - @pnpm/config.package-is-installable@1100.2.2
  - @pnpm/config.version-policy@1100.2.6
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/exec.lifecycle@1100.1.22
  - @pnpm/fs.symlink-dependency@1100.0.23
  - @pnpm/hooks.read-package-hook@1100.3.6
  - @pnpm/installing.context@1101.0.9
  - @pnpm/installing.deps-resolver@1102.2.6
  - @pnpm/installing.deps-restorer@1103.2.3
  - @pnpm/installing.linking.direct-dep-linker@1100.0.23
  - @pnpm/installing.linking.hoist@1100.0.36
  - @pnpm/installing.linking.modules-cleaner@1100.1.28
  - @pnpm/installing.package-requester@1102.2.3
  - @pnpm/lockfile.filtering@1100.2.11
  - @pnpm/lockfile.fs@1100.2.12
  - @pnpm/lockfile.preferred-versions@1100.0.37
  - @pnpm/lockfile.settings-checker@1100.2.11
  - @pnpm/lockfile.to-pnp@1101.0.9
  - @pnpm/lockfile.verification@1100.1.10
  - @pnpm/pkg-manifest.utils@1100.4.8
  - @pnpm/pnpr.client@3.1.2
  - @pnpm/resolving.local-resolver@1101.2.5
  - @pnpm/resolving.npm-resolver@1104.2.4
  - @pnpm/store.controller-types@1101.3.3
  - @pnpm/workspace.project-manifest-reader@1100.1.2
