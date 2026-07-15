## 1100.10.6

### Patch Changes

- Keep the interactive `minimumReleaseAge` approval prompt visible during `pnpm install`. The progress reporter now pauses its redraws while a prompt is waiting for input instead of overwriting it, so the install no longer hangs on a question the user cannot see [#13019](https://github.com/pnpm/pnpm/issues/13019).

- Updated dependencies:
  - @pnpm/building.after-install@1102.0.7
  - @pnpm/cli.utils@1101.0.15
  - @pnpm/config.reader@1101.12.1
  - @pnpm/core-loggers@1100.2.3
  - @pnpm/deps.inspection.outdated@1100.1.15
  - @pnpm/deps.security.signatures@1101.2.5
  - @pnpm/deps.status@1100.1.8
  - @pnpm/global.commands@1100.0.35
  - @pnpm/hooks.pnpmfile@1100.0.20
  - @pnpm/installing.context@1100.0.25
  - @pnpm/installing.deps-installer@1102.3.1
  - @pnpm/installing.env-installer@1102.0.7
  - @pnpm/network.fetch@1100.1.6
  - @pnpm/pkg-manifest.utils@1100.2.8
  - @pnpm/resolving.npm-resolver@1102.1.4
  - @pnpm/store.connection-manager@1100.3.7
  - @pnpm/store.controller@1102.0.6
  - @pnpm/workspace.project-manifest-reader@1100.0.16
  - @pnpm/workspace.projects-filter@1100.0.28
  - @pnpm/workspace.projects-graph@1100.0.24
  - @pnpm/workspace.projects-reader@1101.0.15
  - @pnpm/workspace.state@1100.0.29
