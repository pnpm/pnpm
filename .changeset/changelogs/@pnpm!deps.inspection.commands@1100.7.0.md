## 1100.7.0

### Minor Changes

- Added a new setting, `update.githubActionsServer`, for specifying the base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). When the setting is not defined, the URL is read from the `GITHUB_SERVER_URL` environment variable, falling back to `https://github.com`. The URL must use the `https://` or `http://` protocol [#13220](https://github.com/pnpm/pnpm/issues/13220).

  `pnpm outdated` and `pnpm update` no longer fail when the refs of a GitHub Action's repository cannot be read (for example, when the action's repository is private or hosted on a different GitHub server). Such actions are now skipped with a warning.

  Setting `update.githubActions` to `false` now makes `pnpm outdated` and the interactive `pnpm update` skip GitHub Actions dependencies.

### Patch Changes

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Updated dependencies:
  - @pnpm/cli.command@1100.0.2
  - @pnpm/cli.common-cli-options-help@1100.0.3
  - @pnpm/cli.utils@1101.0.19
  - @pnpm/config.matcher@1100.0.2
  - @pnpm/config.pick-registry-for-package@1100.0.12
  - @pnpm/config.reader@1101.14.0
  - @pnpm/deps.github-actions@1100.1.0
  - @pnpm/deps.inspection.list@1100.0.29
  - @pnpm/deps.inspection.outdated@1100.1.20
  - @pnpm/deps.inspection.peers-checker@1100.0.23
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.9
  - @pnpm/error@1100.1.0
  - @pnpm/global.commands@1100.0.40
  - @pnpm/global.packages@1100.0.13
  - @pnpm/installing.modules-yaml@1100.0.12
  - @pnpm/lockfile.fs@1100.1.14
  - @pnpm/network.auth-header@1101.1.6
  - @pnpm/network.fetch@1100.1.8
  - @pnpm/resolving.default-resolver@1100.3.20
  - @pnpm/resolving.npm-resolver@1102.1.8
  - @pnpm/resolving.registry.types@1100.1.6
  - @pnpm/store.path@1100.0.3
  - @pnpm/types@1101.6.0
  - @pnpm/workspace.project-manifest-reader@1100.0.20
