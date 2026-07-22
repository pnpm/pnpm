## 1100.11.0

### Minor Changes

- Added a `--changeset` flag to `pnpm update`. Set `update.changeset` to `true` in `pnpm-workspace.yaml` to enable this behavior by default, and use `--no-changeset` to override the setting for one update. After the update completes, pnpm writes a `.changeset/pnpm-update-<suffix>.md` file declaring a patch bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update and a major bump when `peerDependencies` changed, including packages that consume an updated catalog entry via the `catalog:` protocol. Private packages, packages without a name, and packages listed in the `ignore` array of `.changeset/config.json` are skipped. If `.changeset/config.json` does not exist, a warning is printed and no changeset is generated.

- Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.

### Patch Changes

- Updated dependencies:
  - @pnpm/building.after-install@1102.0.11
  - @pnpm/building.policy@1100.0.14
  - @pnpm/cli.utils@1101.0.18
  - @pnpm/config.pick-registry-for-package@1100.0.11
  - @pnpm/config.reader@1101.13.0
  - @pnpm/config.version-policy@1100.1.8
  - @pnpm/config.writer@1100.0.17
  - @pnpm/core-loggers@1100.2.4
  - @pnpm/deps.github-actions@1100.0.1
  - @pnpm/deps.inspection.outdated@1100.1.19
  - @pnpm/deps.path@1100.0.10
  - @pnpm/deps.security.signatures@1101.2.6
  - @pnpm/deps.status@1100.1.11
  - @pnpm/global.commands@1100.0.39
  - @pnpm/hooks.pnpmfile@1100.0.21
  - @pnpm/installing.context@1100.0.27
  - @pnpm/installing.dedupe.check@1100.1.4
  - @pnpm/installing.deps-installer@1102.3.5
  - @pnpm/installing.env-installer@1102.0.11
  - @pnpm/lockfile.fs@1100.1.13
  - @pnpm/lockfile.types@1100.0.15
  - @pnpm/network.auth-header@1101.1.5
  - @pnpm/network.fetch@1100.1.7
  - @pnpm/pkg-manifest.reader@1100.0.11
  - @pnpm/pkg-manifest.utils@1100.2.11
  - @pnpm/resolving.npm-resolver@1102.1.7
  - @pnpm/resolving.resolver-base@1100.5.3
  - @pnpm/store.connection-manager@1100.3.11
  - @pnpm/store.controller@1102.0.7
  - @pnpm/types@1101.5.0
  - @pnpm/workspace.project-manifest-reader@1100.0.19
  - @pnpm/workspace.project-manifest-writer@1100.0.10
  - @pnpm/workspace.projects-filter@1100.0.31
  - @pnpm/workspace.projects-graph@1100.0.27
  - @pnpm/workspace.projects-reader@1101.0.18
  - @pnpm/workspace.projects-sorter@1100.0.10
  - @pnpm/workspace.state@1100.0.32
  - @pnpm/workspace.workspace-manifest-reader@1100.1.1
  - @pnpm/workspace.workspace-manifest-writer@1100.0.17
