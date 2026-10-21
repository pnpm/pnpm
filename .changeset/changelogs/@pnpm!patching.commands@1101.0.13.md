## 1101.0.13

### Patch Changes

- Fixed patching in workspaces with `sharedWorkspaceLockfile: false`. `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` now checks configured patches across all workspace projects before reporting unused patches.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.reader@1102.3.6
  - @pnpm/config.writer@1100.0.34
  - @pnpm/fs.packlist@1100.0.9
  - @pnpm/installing.commands@1101.4.7
  - @pnpm/installing.modules-yaml@1101.0.7
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.pruner@1100.0.28
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/pkg-manifest.reader@1100.0.23
  - @pnpm/store.connection-manager@1101.3.5
  - @pnpm/store.path@1100.1.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
  - @pnpm/workspace.workspace-manifest-reader@1100.3.0
