## 1100.7.1

### Patch Changes

- Checking GitHub Actions dependencies for updates is now opt-in for every command. Neither `pnpm outdated` nor `pnpm update` reads the workflow files unless `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Reading them runs `git ls-remote` against every referenced repository, which fails in environments where GitHub is not reachable the way pnpm assumes (a GitHub Enterprise Server, a custom certificate authority, or an offline network) [#13254](https://github.com/pnpm/pnpm/issues/13254).

  `pnpm outdated` accepts the `--include-github-actions` option too.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.20
  - @pnpm/config.pick-registry-for-package@1100.0.13
  - @pnpm/config.reader@1101.15.0
  - @pnpm/deps.github-actions@1100.1.1
  - @pnpm/deps.inspection.list@1100.0.30
  - @pnpm/deps.inspection.outdated@1100.1.21
  - @pnpm/deps.inspection.peers-checker@1100.0.24
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.10
  - @pnpm/global.commands@1100.0.41
  - @pnpm/global.packages@1100.0.14
  - @pnpm/installing.modules-yaml@1100.0.13
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/network.auth-header@1101.1.7
  - @pnpm/network.fetch@1100.1.9
  - @pnpm/resolving.default-resolver@1100.3.21
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/resolving.registry.types@1100.1.7
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.project-manifest-reader@1100.0.21
