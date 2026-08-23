## 1100.1.18

### Patch Changes

- Fixed workspace discovery for `pnpm-workspace.yaml` files without a `packages` field so commands only consider the workspace root instead of recursively scanning nested projects [#14047](https://github.com/pnpm/pnpm/issues/14047).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.4
  - @pnpm/config.reader@1102.0.0
  - @pnpm/constants@1102.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/installing.context@1101.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.settings-checker@1100.2.2
  - @pnpm/lockfile.verification@1100.1.1
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.projects-reader@1101.0.24
  - @pnpm/workspace.state@1100.0.39
  - @pnpm/workspace.workspace-manifest-reader@1100.1.7
