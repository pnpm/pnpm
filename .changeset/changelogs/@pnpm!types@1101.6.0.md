## 1101.6.0

### Minor Changes

- Added a new setting, `update.githubActionsServer`, for specifying the base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). When the setting is not defined, the URL is read from the `GITHUB_SERVER_URL` environment variable, falling back to `https://github.com`. The URL must use the `https://` or `http://` protocol [#13220](https://github.com/pnpm/pnpm/issues/13220).

  `pnpm outdated` and `pnpm update` no longer fail when the refs of a GitHub Action's repository cannot be read (for example, when the action's repository is private or hosted on a different GitHub server). Such actions are now skipped with a warning.

  Setting `update.githubActions` to `false` now makes `pnpm outdated` and the interactive `pnpm update` skip GitHub Actions dependencies.

### Patch Changes

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).
