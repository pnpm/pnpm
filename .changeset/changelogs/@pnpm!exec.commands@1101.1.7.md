## 1101.1.7

### Patch Changes

- `pnpm run -r` now closes the collapsible CI log section of a project whose script fails, so the output of later projects is no longer nested inside it. `--resume-from` no longer crashes when a saved run state file contains `null`.

- Updated dependencies:
  - @pnpm/building.commands@1101.2.7
  - @pnpm/config.reader@1102.3.2
  - @pnpm/deps.status@1100.1.27
  - @pnpm/engine.runtime.commands@1101.1.6
  - @pnpm/exec.lifecycle@1100.1.21
  - @pnpm/exec.npm-lifecycle@1100.0.3
  - @pnpm/installing.client@1100.3.13
  - @pnpm/installing.commands@1101.4.3
  - @pnpm/workspace.injected-deps-syncer@1100.0.41
