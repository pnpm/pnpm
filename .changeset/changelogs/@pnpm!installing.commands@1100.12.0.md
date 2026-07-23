## 1100.12.0

### Minor Changes

- Added a new setting, `update.githubActionsServer`, for specifying the base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). When the setting is not defined, the URL is read from the `GITHUB_SERVER_URL` environment variable, falling back to `https://github.com`. The URL must use the `https://` or `http://` protocol [#13220](https://github.com/pnpm/pnpm/issues/13220).

  `pnpm outdated` and `pnpm update` no longer fail when the refs of a GitHub Action's repository cannot be read (for example, when the action's repository is private or hosted on a different GitHub server). Such actions are now skipped with a warning.

  Setting `update.githubActions` to `false` now makes `pnpm outdated` and the interactive `pnpm update` skip GitHub Actions dependencies.

### Patch Changes

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Updated dependencies:
  - @pnpm/building.after-install@1102.0.12
  - @pnpm/building.policy@1100.0.15
  - @pnpm/catalogs.config@1100.0.3
  - @pnpm/catalogs.protocol-parser@1100.0.1
  - @pnpm/catalogs.types@1100.0.1
  - @pnpm/cli.command@1100.0.2
  - @pnpm/cli.common-cli-options-help@1100.0.3
  - @pnpm/cli.utils@1101.0.19
  - @pnpm/config.matcher@1100.0.2
  - @pnpm/config.pick-registry-for-package@1100.0.12
  - @pnpm/config.reader@1101.14.0
  - @pnpm/config.version-policy@1100.1.9
  - @pnpm/config.writer@1100.0.18
  - @pnpm/constants@1100.0.1
  - @pnpm/core-loggers@1100.2.5
  - @pnpm/deps.github-actions@1100.1.0
  - @pnpm/deps.inspection.outdated@1100.1.20
  - @pnpm/deps.path@1100.0.11
  - @pnpm/deps.security.signatures@1101.2.7
  - @pnpm/deps.status@1100.1.12
  - @pnpm/error@1100.1.0
  - @pnpm/fs.graceful-fs@1100.1.1
  - @pnpm/fs.read-modules-dir@1100.0.2
  - @pnpm/global.commands@1100.0.40
  - @pnpm/hooks.pnpmfile@1100.0.22
  - @pnpm/installing.context@1100.0.28
  - @pnpm/installing.dedupe.check@1100.1.5
  - @pnpm/installing.dedupe.issues-renderer@1100.0.2
  - @pnpm/installing.deps-installer@1102.3.6
  - @pnpm/installing.env-installer@1102.0.12
  - @pnpm/lockfile.fs@1100.1.14
  - @pnpm/lockfile.types@1100.0.16
  - @pnpm/network.auth-header@1101.1.6
  - @pnpm/network.fetch@1100.1.8
  - @pnpm/pkg-manifest.reader@1100.0.12
  - @pnpm/pkg-manifest.utils@1100.2.12
  - @pnpm/resolving.npm-resolver@1102.1.8
  - @pnpm/resolving.parse-wanted-dependency@1100.0.2
  - @pnpm/resolving.resolver-base@1100.5.4
  - @pnpm/store.connection-manager@1100.3.12
  - @pnpm/store.controller@1102.0.8
  - @pnpm/types@1101.6.0
  - @pnpm/workspace.project-manifest-reader@1100.0.20
  - @pnpm/workspace.project-manifest-writer@1100.0.11
  - @pnpm/workspace.projects-filter@1100.0.32
  - @pnpm/workspace.projects-graph@1100.0.28
  - @pnpm/workspace.projects-reader@1101.0.19
  - @pnpm/workspace.projects-sorter@1100.0.11
  - @pnpm/workspace.root-finder@1100.0.4
  - @pnpm/workspace.state@1100.0.33
  - @pnpm/workspace.workspace-manifest-reader@1100.1.2
  - @pnpm/workspace.workspace-manifest-writer@1100.0.18
