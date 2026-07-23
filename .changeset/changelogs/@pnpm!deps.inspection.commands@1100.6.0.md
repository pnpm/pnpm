## 1100.6.0

### Minor Changes

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.18
  - @pnpm/config.pick-registry-for-package@1100.0.11
  - @pnpm/config.reader@1101.13.0
  - @pnpm/deps.github-actions@1100.0.1
  - @pnpm/deps.inspection.list@1100.0.28
  - @pnpm/deps.inspection.outdated@1100.1.19
  - @pnpm/deps.inspection.peers-checker@1100.0.22
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.8
  - @pnpm/global.commands@1100.0.39
  - @pnpm/global.packages@1100.0.12
  - @pnpm/installing.modules-yaml@1100.0.11
  - @pnpm/lockfile.fs@1100.1.13
  - @pnpm/network.auth-header@1101.1.5
  - @pnpm/network.fetch@1100.1.7
  - @pnpm/resolving.default-resolver@1100.3.19
  - @pnpm/resolving.npm-resolver@1102.1.7
  - @pnpm/resolving.registry.types@1100.1.5
  - @pnpm/types@1101.5.0
  - @pnpm/workspace.project-manifest-reader@1100.0.19
