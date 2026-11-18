## 1101.4.12

### Patch Changes

- `pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`. `pnpm clean` and `pnpm ci` also clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.

- `pnpm -r fetch` now fetches each selected project's lockfile when `sharedWorkspaceLockfile` is `false`. Use `--filter` to limit which projects are fetched [pnpm/pnpm#18152](https://github.com/pnpm/pnpm/issues/18152).

- `pnpm dlx` now keys its cache on active `overrides`, `packageExtensions`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

  `pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself. Denied packages also skip interactive build approval [#18075](https://github.com/pnpm/pnpm/pull/18075).

- Fixed GitHub Actions support in `pnpm update` and `pnpm outdated`. Updating a workflow step with a major or minor version comment such as `# v4` or a prose comment now formats the updated version comment cleanly. Negated action selectors such as `!actions/cache/save` now exclude only the targeted subpath actions. Passing only negated action selectors still updates packages. Previously, negated action selectors skipped package updates. `pnpm outdated` now reports every distinct outdated version when several workflows pin the same action at different versions.

- `pnpm import` now preserves locked versions for `npm:` aliased dependencies when importing from `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock`. `pnpm import` also parses Yarn Classic `yarn.lock` files that reference `__metadata` package names without treating them as Yarn Berry lockfiles.

- `pnpm update --changeset` now generates a changeset when a `.changeset` directory exists or `versioning` is configured in `pnpm-workspace.yaml`, without requiring `.changeset/config.json`. It also respects `versioning.ignore` alongside `.changeset/config.json`.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.17
  - @pnpm/building.policy@1100.1.11
  - @pnpm/cli.utils@1101.0.36
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/config.pick-registry-for-package@1101.0.6
  - @pnpm/config.reader@1102.3.10
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/config.writer@1100.0.38
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.github-actions@1100.1.20
  - @pnpm/deps.inspection.outdated@1100.1.43
  - @pnpm/deps.path@1101.0.10
  - @pnpm/deps.security.signatures@1102.0.13
  - @pnpm/deps.status@1100.1.36
  - @pnpm/global.commands@1102.0.14
  - @pnpm/global.packages@1101.1.12
  - @pnpm/hooks.pnpmfile@1100.0.40
  - @pnpm/installing.context@1101.0.17
  - @pnpm/installing.dedupe.check@1100.1.20
  - @pnpm/installing.deps-installer@1104.2.11
  - @pnpm/installing.deps-resolver@1102.2.14
  - @pnpm/installing.env-installer@1103.0.17
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/network.auth-header@1101.1.19
  - @pnpm/network.fetch@1100.1.26
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/releasing.versioning@1100.3.10
  - @pnpm/resolving.local-resolver@1101.2.10
  - @pnpm/resolving.npm-resolver@1104.2.11
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/store.connection-manager@1101.3.10
  - @pnpm/store.controller@1102.2.11
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.injected-deps-syncer@1100.0.50
  - @pnpm/workspace.project-manifest-reader@1100.1.7
  - @pnpm/workspace.project-manifest-writer@1100.0.23
  - @pnpm/workspace.projects-filter@1100.0.55
  - @pnpm/workspace.projects-graph@1100.0.49
  - @pnpm/workspace.projects-reader@1101.1.8
  - @pnpm/workspace.projects-sorter@1101.0.5
  - @pnpm/workspace.root-finder@1100.1.7
  - @pnpm/workspace.state@1100.0.56
  - @pnpm/workspace.task-scheduler@1100.0.8
  - @pnpm/workspace.workspace-manifest-reader@1100.3.3
  - @pnpm/workspace.workspace-manifest-writer@1100.2.12
