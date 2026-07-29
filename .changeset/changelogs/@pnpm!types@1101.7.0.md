## 1101.7.0

### Minor Changes

- Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. It is for a project whose published name is already taken by a sibling project, which otherwise has to be renamed by a build step just before publishing. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches the packed manifest, the tarball filename, and everything that addresses the package at the registry: the already-published check of `pnpm publish -r`, its registry selection, and the release-planning probes of `pnpm change status` and `pnpm version -r` [#13345](https://github.com/pnpm/pnpm/issues/13345).

### Patch Changes

- Checking GitHub Actions dependencies for updates is now opt-in for every command. Neither `pnpm outdated` nor `pnpm update` reads the workflow files unless `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Reading them runs `git ls-remote` against every referenced repository, which fails in environments where GitHub is not reachable the way pnpm assumes (a GitHub Enterprise Server, a custom certificate authority, or an offline network) [#13254](https://github.com/pnpm/pnpm/issues/13254).

  `pnpm outdated` accepts the `--include-github-actions` option too.
