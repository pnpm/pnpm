## 1100.4.5

### Patch Changes

- `pnpm -r run "/pattern/" --no-bail` no longer exits zero when one of a project's matched scripts fails and a later one passes. The run summary carries a single status per project, and the passing script overwrote the recorded failure.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.12
  - @pnpm/building.commands@1100.1.18
  - @pnpm/cli.utils@1101.0.20
  - @pnpm/config.reader@1101.15.0
  - @pnpm/config.version-policy@1100.1.10
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.status@1100.1.13
  - @pnpm/engine.runtime.commands@1100.1.17
  - @pnpm/exec.lifecycle@1100.1.9
  - @pnpm/installing.client@1100.3.0
  - @pnpm/installing.commands@1100.12.1
  - @pnpm/pkg-manifest.reader@1100.0.13
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.injected-deps-syncer@1100.0.29
  - @pnpm/workspace.project-manifest-reader@1100.0.21
  - @pnpm/workspace.projects-sorter@1100.0.12
