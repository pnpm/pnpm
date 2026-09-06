## 1101.2.1

### Patch Changes

- Build scripts can now be rejected before the package is installed [#14067](https://github.com/pnpm/pnpm/issues/14067):

  - `pnpm add --allow-build=!<pkg>` records `<pkg>: false` in `allowBuilds`. It used to write a `!<pkg>: true` entry that matched no package. On a global install the denial was dropped altogether, so the post-install prompt offered the build for approval.
  - `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` record their decision even when no packages are awaiting approval, and report a package that is not awaiting approval with a warning so a typo stays visible. Both cases used to fail the command with an error.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.3
  - @pnpm/building.policy@1100.1.0
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/config.writer@1100.0.25
  - @pnpm/error@1100.1.4
  - @pnpm/global.packages@1101.1.1
  - @pnpm/installing.commands@1101.2.0
  - @pnpm/store.connection-manager@1101.1.1
  - @pnpm/workspace.task-scheduler@1100.0.1
  - @pnpm/workspace.workspace-manifest-reader@1100.1.9
