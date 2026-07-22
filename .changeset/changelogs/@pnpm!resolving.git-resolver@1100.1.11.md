## 1100.1.11

### Patch Changes

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

- Updated dependencies:
  - @pnpm/network.fetch@1100.1.7
  - @pnpm/resolving.resolver-base@1100.5.3
