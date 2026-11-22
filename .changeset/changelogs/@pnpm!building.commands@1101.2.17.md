## 1101.2.17

### Patch Changes

- `pnpm approve-builds` now requires the exact selector shown by `pnpm ignored-builds` for a dependency a bare package name cannot identify (a `file:`, git, or tarball artifact). A bare package name only approves a plain registry version [#18237](https://github.com/pnpm/pnpm/issues/18237).

- Internal type-safety cleanup with no change in behavior.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.18
  - @pnpm/building.policy@1100.1.12
  - @pnpm/cli.common-cli-options-help@1100.0.4
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.reader@1102.4.0
  - @pnpm/config.writer@1100.0.39
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/global.packages@1101.1.13
  - @pnpm/installing.commands@1101.4.13
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/store.connection-manager@1101.3.11
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.projects-sorter@1101.0.6
  - @pnpm/workspace.task-scheduler@1100.0.9
  - @pnpm/workspace.workspace-manifest-reader@1100.3.4
