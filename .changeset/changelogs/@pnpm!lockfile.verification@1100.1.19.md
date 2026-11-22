## 1100.1.19

### Patch Changes

- `pnpm install --frozen-lockfile` now accepts a plain-range dependency of a local directory dependency that `linkWorkspacePackages: deep` linked to a workspace project, and rejects the lockfile when its `injected` setting no longer matches the recorded reference.

- `pnpm install --frozen-lockfile` now fails when the `injected` setting of a dependency of a local directory dependency no longer matches the lockfile [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/crypto.hash@1100.0.10
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/installing.context@1101.0.18
  - @pnpm/installing.deps-resolver@1102.3.0
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/resolving.git-resolver@1100.1.32
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
