## 1100.1.23

### Patch Changes

- `pnpm install` now returns "Already up to date" in a workspace where `dedupeDirectDeps` left a project without a `node_modules` directory of its own. Such a project forced a full install on every run.

- Updated dependencies:
  - @pnpm/config.reader@1102.2.1
  - @pnpm/installing.context@1101.0.5
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.settings-checker@1100.2.7
  - @pnpm/lockfile.verification@1100.1.6
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/workspace.projects-reader@1101.0.28
  - @pnpm/workspace.state@1100.0.44
