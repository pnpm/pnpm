## 1102.0.0

### Major Changes

- Build scripts can now be rejected before the package is installed [#14067](https://github.com/pnpm/pnpm/issues/14067):

  - `pnpm add --allow-build=!<pkg>` records `<pkg>: false` in `allowBuilds`. It used to write a `!<pkg>: true` entry that matched no package. On a global install the denial was dropped altogether, so the post-install prompt offered the build for approval.
  - `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` record their decision even when no packages are awaiting approval, and report a package that is not awaiting approval with a warning so a typo stays visible. Both cases used to fail the command with an error.

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.30
  - @pnpm/bins.remover@1100.0.23
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/deps.inspection.list@1101.0.3
  - @pnpm/error@1100.1.4
  - @pnpm/global.packages@1101.1.1
  - @pnpm/installing.deps-installer@1104.1.1
  - @pnpm/pkg-manifest.reader@1100.0.19
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/store.connection-manager@1101.1.1
