## 1100.1.1

### Patch Changes

- Checking GitHub Actions dependencies for updates is now opt-in for every command. Neither `pnpm outdated` nor `pnpm update` reads the workflow files unless `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Reading them runs `git ls-remote` against every referenced repository, which fails in environments where GitHub is not reachable the way pnpm assumes (a GitHub Enterprise Server, a custom certificate authority, or an offline network) [#13254](https://github.com/pnpm/pnpm/issues/13254).

  `pnpm outdated` accepts the `--include-github-actions` option too.

- Updated dependencies:
  - @pnpm/resolving.git-resolver@1100.1.13
