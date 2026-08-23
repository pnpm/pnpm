## 1100.0.28

### Patch Changes

- On Windows, upgrading pnpm no longer leaves a stale `pnpm.ps1` behind. PowerShell resolves `pnpm.ps1` ahead of `pnpm.cmd`, so a shim written by an older installation kept running the previous version. Linking the pnpm CLI's bins now deletes it [#13919](https://github.com/pnpm/pnpm/issues/13919).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.15
  - @pnpm/error@1100.1.3
  - @pnpm/pkg-manifest.reader@1100.0.17
  - @pnpm/pkg-manifest.utils@1100.4.1
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
