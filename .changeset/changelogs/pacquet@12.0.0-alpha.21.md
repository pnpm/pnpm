## 12.0.0-alpha.21

### Minor Changes

- `pnpm setup` now appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands [#9191](https://github.com/pnpm/pnpm/issues/9191).

### Patch Changes

- Checking GitHub Actions dependencies for updates is now opt-in for every command. Neither `pnpm outdated` nor `pnpm update` reads the workflow files unless `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Reading them runs `git ls-remote` against every referenced repository, which fails in environments where GitHub is not reachable the way pnpm assumes (a GitHub Enterprise Server, a custom certificate authority, or an offline network) [#13254](https://github.com/pnpm/pnpm/issues/13254).

  `pnpm outdated` accepts the `--include-github-actions` option too.

- `pnpm update --latest` now keeps dependencies that the npm registry does not serve in the form they were declared. A `runtime:` dependency (such as `"node": "runtime:26.5.0"`), a `git`/`github:` URL, or a remote tarball URL previously had its *name* looked up on the npm registry and its specifier overwritten with that unrelated package's version.

  `pnpm update --latest` also no longer rewrites `package.json` when a dependency is already at its latest version.
